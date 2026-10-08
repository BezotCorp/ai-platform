// On riscv64, update() bails early because no release artifacts are published,
// so the implementation below (and most of this module) is cfg-disabled and
// would otherwise trigger dead_code/unused warnings that fail clippy.
#![cfg_attr(
    target_arch = "riscv64",
    allow(dead_code, unused_imports, unused_variables)
)]

use anyhow::{Context, Result, bail};
use reqwest::{
    StatusCode,
    header::{AUTHORIZATION, HeaderValue},
};
use sha2::{Digest, Sha256};
use sigstore_verify::VerificationPolicy;
use sigstore_verify::trust_root::{SIGSTORE_PRODUCTION_TRUSTED_ROOT, TrustedRoot};
use sigstore_verify::types::{Bundle, Sha256Hash};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::{env, fs};
/// Asset name for this platform (compile-time).
fn asset_name() -> &'static str {
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    {
        "bcaip-aarch64-apple-darwin.tar.bz2"
    }
    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    {
        "bcaip-x86_64-apple-darwin.tar.bz2"
    }
    #[cfg(all(target_os = "linux", target_arch = "x86_64", target_env = "gnu"))]
    {
        "bcaip-x86_64-unknown-linux-gnu.tar.bz2"
    }
    #[cfg(all(target_os = "linux", target_arch = "aarch64", target_env = "gnu"))]
    {
        "bcaip-aarch64-unknown-linux-gnu.tar.bz2"
    }
    #[cfg(all(target_os = "linux", target_arch = "x86_64", target_env = "musl"))]
    {
        "bcaip-x86_64-unknown-linux-musl.tar.bz2"
    }
    #[cfg(all(target_os = "linux", target_arch = "aarch64", target_env = "musl"))]
    {
        "bcaip-aarch64-unknown-linux-musl.tar.bz2"
    }
    // RISC-V builds compile with this asset name, but update() rejects the
    // platform until release artifacts are published. See update() below.
    #[cfg(all(target_os = "linux", target_arch = "riscv64", target_env = "gnu"))]
    {
        "bcaip-riscv64gc-unknown-linux-gnu.tar.bz2"
    }
    #[cfg(all(target_os = "windows", target_arch = "x86_64", feature = "cuda"))]
    {
        "bcaip-x86_64-pc-windows-msvc-cuda.zip"
    }
    #[cfg(all(target_os = "windows", target_arch = "x86_64", not(feature = "cuda")))]
    {
        "bcaip-x86_64-pc-windows-msvc.zip"
    }
}

/// Binary name for this platform.
fn binary_name() -> &'static str {
    #[cfg(target_os = "windows")]
    {
        "bcaip.exe"
    }
    #[cfg(not(target_os = "windows"))]
    {
        "bcaip"
    }
}

// ---------------------------------------------------------------------------
// Sigstore / SLSA provenance verification
// ---------------------------------------------------------------------------

/// Compute the SHA-256 hex digest of a byte slice.
fn sha256_hex(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    bcaip::utils::bytes_to_hex(hasher.finalize())
}

#[derive(serde::Deserialize)]
struct AttestationResponse {
    attestations: Vec<AttestationEntry>,
}

#[derive(serde::Deserialize)]
struct AttestationEntry {
    #[serde(default)]
    bundle: Option<serde_json::Value>,
    #[serde(default)]
    bundle_url: Option<String>,
}

const GITHUB_ACTIONS_ISSUER: &str = "https://token.actions.githubusercontent.com";

fn sanitized_token(token: Option<&str>) -> Option<&str> {
    token.map(str::trim).filter(|tok| !tok.is_empty())
}

fn authorization_header_value(token: &str) -> Option<HeaderValue> {
    HeaderValue::from_str(&format!("Bearer {token}")).ok()
}

fn github_token() -> Option<String> {
    env::var("GITHUB_TOKEN")
        .ok()
        .and_then(|tok| sanitized_token(Some(&tok)).map(str::to_owned))
        .or_else(|| {
            env::var("GH_TOKEN")
                .ok()
                .and_then(|tok| sanitized_token(Some(&tok)).map(str::to_owned))
        })
}

fn should_retry_attestations_without_token(status: StatusCode, token: Option<&str>) -> bool {
    sanitized_token(token)
        .and_then(authorization_header_value)
        .is_some()
        && matches!(status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN)
}

async fn fetch_attestations(digest: &str, token: Option<&str>) -> Result<Vec<serde_json::Value>> {
    let url = format!(
        "https://api.github.com/repos/BezotCorp/ai-platform/attestations/sha256:{digest}\
         ?per_page=30&predicate_type=https://slsa.dev/provenance/v1"
    );

    let client = reqwest::Client::new();
    let token = sanitized_token(token);
    let resp = fetch_attestations_response(&client, &url, token).await?;

    let resp = if should_retry_attestations_without_token(resp.status(), token) {
        fetch_attestations_response(&client, &url, None).await?
    } else {
        resp
    };

    if !resp.status().is_success() {
        bail!("GitHub attestation API returned HTTP {}", resp.status());
    }

    let body: AttestationResponse = resp
        .json()
        .await
        .context("Failed to parse attestation response")?;

    // GitHub no longer embeds bundles in attestation list responses; they are
    // served from blob storage via bundle_url instead.
    let mut bundles = Vec::with_capacity(body.attestations.len());
    for entry in body.attestations {
        match (entry.bundle, entry.bundle_url) {
            (Some(bundle), _) if !bundle.is_null() => bundles.push(bundle),
            (_, Some(url)) => bundles.push(fetch_bundle(&client, &url).await?),
            _ => bail!("Attestation has neither a bundle nor a bundle URL"),
        }
    }

    Ok(bundles)
}

// The bundle URL is pre-signed for blob storage, so no API credentials are sent.
async fn fetch_bundle(client: &reqwest::Client, url: &str) -> Result<serde_json::Value> {
    let resp = client
        .get(url)
        .header("User-Agent", "bcaip-cli")
        .send()
        .await
        .context("Failed to fetch attestation bundle")?;

    if !resp.status().is_success() {
        bail!(
            "Attestation bundle download returned HTTP {}",
            resp.status()
        );
    }

    let body = resp
        .bytes()
        .await
        .context("Failed to read attestation bundle")?;
    parse_bundle_bytes(&body)
}

fn parse_bundle_bytes(body: &[u8]) -> Result<serde_json::Value> {
    if let Ok(bundle) = serde_json::from_slice(body) {
        return Ok(bundle);
    }

    // Offloaded bundles are served as snappy-compressed JSON.
    let decompressed = snap::raw::Decoder::new()
        .decompress_vec(body)
        .context("Failed to decompress attestation bundle")?;
    serde_json::from_slice(&decompressed).context("Failed to parse attestation bundle")
}

async fn fetch_attestations_response(
    client: &reqwest::Client,
    url: &str,
    token: Option<&str>,
) -> Result<reqwest::Response> {
    let mut req = client
        .get(url)
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .header("User-Agent", "bcaip-cli");

    if let Some(value) = token.and_then(authorization_header_value) {
        req = req.header(AUTHORIZATION, value);
    }

    req.send().await.context("Failed to fetch attestations")
}

// Verify a single attestation bundle against the artifact digest and workflow.
fn verify_bundle(
    bundle_json: &serde_json::Value,
    artifact_digest: Sha256Hash,
    policy: &VerificationPolicy,
    trusted_root: &TrustedRoot,
    workflow: &str,
) -> Result<()> {
    let bundle_str = serde_json::to_string(bundle_json)?;
    let bundle = Bundle::from_json(&bundle_str)
        .map_err(|e| anyhow::anyhow!("Failed to parse bundle: {e}"))?;

    let result = sigstore_verify::verify(artifact_digest, &bundle, policy, trusted_root)
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    let identity = result
        .identity()
        .map(|identity| identity.as_str())
        .ok_or_else(|| anyhow::anyhow!("No identity in certificate"))?;

    let expected = format!("/.github/workflows/{workflow}");
    if !identity.contains(&expected) {
        bail!("Workflow mismatch: expected {workflow}, got {identity}");
    }

    Ok(())
}

/// Returns `Ok(())` when the downloaded archive has verified provenance.
async fn verify_provenance(archive_data: &[u8], tag: &str) -> Result<()> {
    let digest = sha256_hex(archive_data);
    println!("Archive SHA-256: {digest}");

    let workflow = match tag {
        "canary" => "canary.yml",
        _ => "release.yml",
    };

    let token = github_token();

    println!("Verifying SLSA provenance via Sigstore...");

    let bundles = fetch_attestations(&digest, token.as_deref())
        .await
        .context(
            "Sigstore provenance check could not complete; refusing to install unverifiable update",
        )?;

    if bundles.is_empty() {
        bail!(
            "No Sigstore attestation found for downloaded archive; refusing to install unverifiable update"
        );
    }

    let trusted_root = TrustedRoot::from_json(SIGSTORE_PRODUCTION_TRUSTED_ROOT)
        .context("Failed to load Sigstore trusted root")?;
    let policy = VerificationPolicy::any_identity().require_issuer(GITHUB_ACTIONS_ISSUER);
    let artifact_digest =
        Sha256Hash::from_hex(&digest).context("Failed to parse artifact digest")?;

    // One passing attestation is sufficient.
    let mut last_err = None;
    for bundle_json in &bundles {
        match verify_bundle(
            bundle_json,
            artifact_digest,
            &policy,
            &trusted_root,
            workflow,
        ) {
            Ok(()) => {
                println!("Sigstore provenance verification passed.");
                return Ok(());
            }
            Err(e) => last_err = Some(e),
        }
    }

    Err(anyhow::anyhow!(
        "Sigstore verification failed: {}\n\nAborting update due to security check failure.",
        last_err.unwrap()
    ))
}

/// Update the BCAIP binary to the latest release.
///
/// Downloads the platform-appropriate archive from GitHub releases, verifies
/// its SLSA provenance via Sigstore, extracts it with path-traversal
/// hardening, and replaces the current binary in-place.
pub async fn update(canary: bool, reconfigure: bool) -> Result<()> {
    #[cfg(feature = "disable-update")]
    {
        bail!("Update is disabled in this build.");
    }

    // RISC-V release artifacts are not published yet, so reject self-update
    // rather than downloading a nonexistent asset.
    #[cfg(all(target_arch = "riscv64", not(feature = "disable-update")))]
    {
        bail!(
            "Self-update is not supported on riscv64: no release artifacts are published for this platform."
        );
    }

    #[cfg(all(not(target_arch = "riscv64"), not(feature = "disable-update")))]
    {
        let tag = if canary { "canary" } else { "stable" };
        let asset = asset_name();
        let url =
            format!("https://github.com/BezotCorp/ai-platform/releases/download/{tag}/{asset}");

        println!("Downloading {asset} from {tag} release...");

        // --- Download -----------------------------------------------------------
        let response = reqwest::get(&url)
            .await
            .context("Failed to download release archive")?;

        if !response.status().is_success() {
            bail!(
                "Download failed with HTTP status {}. URL: {}",
                response.status(),
                url
            );
        }

        let bytes = response
            .bytes()
            .await
            .context("Failed to read response body")?;

        println!("Downloaded {} bytes.", bytes.len());

        // --- Verify SLSA provenance via Sigstore --------------------------------
        verify_provenance(&bytes, tag).await?;

        // --- Extract to temp dir (hardened against path traversal) --------------
        let tmp_dir = tempfile::tempdir().context("Failed to create temp directory")?;

        #[cfg(target_os = "windows")]
        extract_zip(&bytes, tmp_dir.path())?;

        #[cfg(not(target_os = "windows"))]
        extract_tar_bz2(&bytes, tmp_dir.path())?;

        // --- Locate the binary in the extracted archive -------------------------
        let binary = binary_name();
        let extracted_binary = find_binary(tmp_dir.path(), binary)
            .with_context(|| format!("Could not find {binary} in extracted archive"))?;

        // --- Replace the current binary -----------------------------------------
        let current_exe =
            env::current_exe().context("Failed to determine current executable path")?;

        replace_binary(&extracted_binary, &current_exe)
            .context("Failed to replace current binary")?;

        // --- Copy DLLs on Windows -----------------------------------------------
        #[cfg(target_os = "windows")]
        copy_dlls(&extracted_binary, &current_exe)?;

        println!("BCAIP updated successfully (verified with Sigstore SLSA provenance).");

        // --- Reconfigure if requested -------------------------------------------
        if reconfigure {
            println!("Running bcaip configure...");
            let status = Command::new(current_exe)
                .arg("configure")
                .status()
                .context("Failed to run bcaip configure")?;
            if !status.success() {
                eprintln!("Warning: bcaip configure exited with {status}");
            }
        }

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Archive extraction
// ---------------------------------------------------------------------------

/// Extract a .zip archive with path-traversal hardening (Windows).
///
/// Iterates entries individually and uses `enclosed_name()` to reject any
/// path that escapes the destination directory (zip-slip protection).
#[cfg(target_os = "windows")]
fn extract_zip(data: &[u8], dest: &Path) -> Result<()> {
    use std::io::Cursor;
    let cursor = Cursor::new(data);
    let mut archive = zip::ZipArchive::new(cursor).context("Failed to open zip archive")?;

    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .with_context(|| format!("Failed to read zip entry at index {i}"))?;

        let safe_path = match entry.enclosed_name() {
            Some(p) => p.to_owned(),
            None => bail!("Zip entry has unsafe path: {}", entry.name()),
        };

        let target = dest.join(&safe_path);

        if entry.is_dir() {
            fs::create_dir_all(&target)?;
        } else {
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut out = fs::File::create(&target)?;
            std::io::copy(&mut entry, &mut out)?;
        }
    }

    Ok(())
}

/// Validate that an archive entry path is safe (no absolute paths, no `..`).
fn validate_entry_path(path: &Path) -> Result<()> {
    if path.is_absolute() {
        bail!("Tar entry has absolute path: {}", path.display());
    }
    for component in path.components() {
        if matches!(component, std::path::Component::ParentDir) {
            bail!("Tar entry contains path traversal: {}", path.display());
        }
    }
    Ok(())
}

/// Extract a .tar.bz2 archive with path-traversal hardening (macOS / Linux).
///
/// Iterates entries individually, rejecting any entry whose path is absolute
/// or contains `..` components (tar-slip protection).
#[cfg(not(target_os = "windows"))]
fn extract_tar_bz2(data: &[u8], dest: &Path) -> Result<()> {
    use bzip2::read::BzDecoder;
    let decoder = BzDecoder::new(data);
    let mut archive = tar::Archive::new(decoder);

    for entry in archive.entries().context("Failed to read tar entries")? {
        let mut entry = entry.context("Failed to read tar entry")?;
        let path = entry
            .path()
            .context("Failed to read entry path")?
            .into_owned();

        validate_entry_path(&path)?;

        // Block symlinks and hardlinks whose targets escape the destination directory.
        // Use entry.link_name() (not entry.header().link_name()) so GNU/PAX extended
        // metadata (linkpath) is resolved; the header field alone may be truncated.
        let link_target_opt = entry
            .link_name()
            .context("Failed to read link name from tar entry")?;
        if let Some(link_target) = link_target_opt {
            validate_entry_path(&link_target)?;
        }

        let target = dest.join(&path);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }

        entry
            .unpack(&target)
            .with_context(|| format!("Failed to extract: {}", path.display()))?;
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Binary location
// ---------------------------------------------------------------------------

/// Find the binary inside the extracted archive.
///
/// The archive may place it in:
///   1. A `bcaip-package/` subdirectory (Windows releases)
///   2. Directly at the top level
///   3. In some other single subdirectory
fn find_binary(extract_dir: &Path, binary_name: &str) -> Option<PathBuf> {
    // 1. Check bcaip-package subdir (matches download_cli.sh / download_cli.ps1)
    let package_dir = extract_dir.join("bcaip-package");
    if package_dir.is_dir() {
        let p = package_dir.join(binary_name);
        if p.exists() {
            return Some(p);
        }
    }

    // 2. Check top level
    let p = extract_dir.join(binary_name);
    if p.exists() {
        return Some(p);
    }

    // 3. Search one level of subdirectories
    if let Ok(entries) = fs::read_dir(extract_dir) {
        for entry in entries.flatten() {
            if entry.path().is_dir() {
                let candidate = entry.path().join(binary_name);
                if candidate.exists() {
                    return Some(candidate);
                }
            }
        }
    }

    None
}

// ---------------------------------------------------------------------------
// Binary replacement
// ---------------------------------------------------------------------------

/// Replace the current binary with the newly downloaded one.
///
/// On Windows we must rename the running exe (Windows allows rename but not
/// delete/overwrite of a locked file) then copy the new file in.
///
/// On Unix we can simply copy over the existing binary.
fn replace_binary(new_binary: &Path, current_exe: &Path) -> Result<()> {
    #[cfg(target_os = "windows")]
    {
        let old_exe = current_exe.with_extension("exe.old");

        // Clean up leftover from a previous update
        if old_exe.exists() {
            fs::remove_file(&old_exe).with_context(|| {
                format!(
                    "Failed to remove old backup {}. Is another BCAIP process running?",
                    old_exe.display()
                )
            })?;
        }

        // Rename the running binary out of the way
        fs::rename(current_exe, &old_exe).with_context(|| {
            format!(
                "Failed to rename running binary to {}. Try closing BCAIP Desktop if it's open.",
                old_exe.display()
            )
        })?;

        // Copy the new binary into place
        fs::copy(new_binary, current_exe).with_context(|| {
            // Try to restore the old binary
            let _ = fs::rename(&old_exe, current_exe);
            format!("Failed to copy new binary to {}", current_exe.display())
        })?;
    }

    #[cfg(not(target_os = "windows"))]
    {
        let old_exe = current_exe.with_extension("old");

        // Rename current binary to avoid ETXTBSY on Linux
        if current_exe.exists() {
            fs::rename(current_exe, &old_exe).with_context(|| {
                format!("Failed to rename {} before update", current_exe.display())
            })?;
        }

        if let Err(e) = fs::copy(new_binary, current_exe) {
            // Restore old binary if copy fails
            let _ = fs::rename(&old_exe, current_exe);
            return Err(e).with_context(|| {
                format!("Failed to copy new binary to {}", current_exe.display())
            });
        }

        // Delete the old backup binary
        let _ = fs::remove_file(&old_exe);

        // Ensure the binary is executable
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(current_exe)?.permissions();
            perms.set_mode(0o755);
            fs::set_permissions(current_exe, perms)?;
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// DLL handling (Windows only)
// ---------------------------------------------------------------------------

/// Copy any .dll files from the extracted archive alongside the installed binary.
#[cfg(target_os = "windows")]
fn copy_dlls(extracted_binary: &Path, current_exe: &Path) -> Result<()> {
    let source_dir = extracted_binary
        .parent()
        .context("Extracted binary has no parent directory")?;
    let dest_dir = current_exe
        .parent()
        .context("Current executable has no parent directory")?;

    if let Ok(entries) = fs::read_dir(source_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if let Some(ext) = path.extension() {
                if ext.eq_ignore_ascii_case("dll") {
                    let file_name = path.file_name().unwrap();
                    let dest = dest_dir.join(file_name);
                    // Remove existing DLL first (it may be locked by another process)
                    if dest.exists() {
                        let _ = fs::remove_file(&dest);
                    }
                    fs::copy(&path, &dest).with_context(|| {
                        format!("Failed to copy {} to {}", path.display(), dest.display())
                    })?;
                    println!("  Copied {}", file_name.to_string_lossy());
                }
            }
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------
