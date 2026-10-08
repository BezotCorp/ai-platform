use anyhow::{Result, bail};
use futures::StreamExt;
use hf_hub::HFClient;
use hf_hub::progress::{DownloadEvent, FileStatus, ProgressEvent, ProgressHandler};
use hf_hub::repository::{ModelInfo, RepoSibling};
use hf_hub::{HFRepository, RepoTypeModel};

use crate::hf::{ResolvedLocalModel, ResolvedModel, canonicalize_quantization, parse_model_spec, resolve_model_spec_full};
use crate::hf::gguf_catalog::{build_download_url, is_shard_file, parse_quantization};
use crate::hf::gguf_variants::{group_into_variants, is_auxiliary_gguf_file};
use crate::hf::hub_client::{hf_client, model_repo, split_repo_id};
use crate::hf::repo_siblings::get_repo_siblings;
use crate::{HfGgufFile, HfModelInfo, HfModelVariant, HfQuantVariant, download_manager};
use std::path::{self, PathBuf};
use std::sync::{Arc, Mutex};
use std::{collections, fs};

use crate::local_model_format::LLAMACPP_BACKEND_ID;
use crate::local_model_format::model_id_from_repo;


const MLX_BACKEND_ID: &str = "mlx";
const MLX_VARIANT_ID: &str = "default";

pub async fn search_local_models(query: &str, limit: usize) -> Result<Vec<HfModelInfo>> {
    let mut results = Vec::new();

    if looks_like_repo_id(query) {
        if let Some(model) = get_local_model_info_for_repo(query).await? {
            results.push(model);
        }
    } else if let Some(model) = get_exact_name_local_model_info(query).await? {
        results.push(model);
    }

    let mut gguf_results = search_gguf_models(query, limit).await?;
    for model in &mut gguf_results {
        let gguf_variants = get_repo_gguf_variants(&model.repo_id)
            .await
            .unwrap_or_default();
        model.variants = gguf_variants
            .iter()
            .map(|variant| variant.to_model_variant(&model.repo_id))
            .collect();
    }

    results.extend(gguf_results);
    append_optional_mlx_results(&mut results, search_mlx_models(query, limit).await, query);
    dedupe_models(&mut results);
    results.sort_by(|a, b| {
        model_search_rank(query, a)
            .cmp(&model_search_rank(query, b))
            .then_with(|| b.downloads.cmp(&a.downloads))
    });
    results.truncate(limit);
    Ok(results)
}

fn append_optional_mlx_results(
    results: &mut Vec<HfModelInfo>,
    mlx_results: Result<Vec<HfModelInfo>>,
    query: &str,
) {
    match mlx_results {
        Ok(models) => results.extend(models),
        Err(error) => tracing::warn!(
            query,
            error = %error,
            "Failed to search MLX models; returning non-MLX results"
        ),
    }
}

pub async fn search_gguf_models(query: &str, limit: usize) -> Result<Vec<HfModelInfo>> {
    let client = hf_client().await?;
    let stream = client
        .list_models()
        .search(query.to_string())
        .filter("gguf".to_string())
        .sort("downloads".to_string())
        .limit(limit)
        .send()?;
    futures::pin_mut!(stream);
    let mut results = Vec::new();
    while let Some(model) = stream.next().await {
        let model = model?;
        let repo_id = model.id;
        let gguf_files = model
            .siblings
            .unwrap_or_default()
            .into_iter()
            .filter(|sibling| sibling.rfilename.ends_with(".gguf"))
            .map(|sibling| HfGgufFile {
                quantization: parse_quantization(&sibling.rfilename),
                download_url: build_download_url(&repo_id, &sibling.rfilename),
                filename: sibling.rfilename,
                size_bytes: sibling.size.unwrap_or(0),
            })
            .collect();
        let author = model
            .author
            .unwrap_or_else(|| repo_id.split('/').next().unwrap_or_default().to_string());
        let model_name = repo_id
            .split('/')
            .next_back()
            .unwrap_or(&repo_id)
            .to_string();
        results.push(HfModelInfo {
            repo_id,
            author,
            model_name,
            downloads: model.downloads.unwrap_or(0),
            gguf_files,
            variants: Vec::new(),
        });
    }
    Ok(results)
}

/// Fetch GGUF files for a repo and return them grouped by quantization.
pub async fn get_repo_gguf_variants(repo_id: &str) -> Result<Vec<HfQuantVariant>> {
    group_into_variants(repo_id, get_repo_siblings(repo_id).await?)
}

/// Fetch raw GGUF files (kept for resolve_model_spec).
pub async fn get_repo_gguf_files(repo_id: &str) -> Result<Vec<HfGgufFile>> {
    let files = get_repo_siblings(repo_id)
        .await?
        .into_iter()
        .filter(|s| s.rfilename.ends_with(".gguf"))
        .filter(|s| !is_shard_file(&s.rfilename))
        .filter(|s| !is_auxiliary_gguf_file(&s.rfilename))
        .map(|s| {
            let quantization = parse_quantization(&s.rfilename);
            let download_url = build_download_url(repo_id, &s.rfilename);
            HfGgufFile {
                filename: s.rfilename,
                size_bytes: s.size.unwrap_or(0),
                quantization,
                download_url,
            }
        })
        .collect();

    Ok(files)
}

async fn search_mlx_models(query: &str, limit: usize) -> Result<Vec<HfModelInfo>> {
    let mut results = search_mlx_models_with_query(query, limit).await?;
    if !query.contains('/') {
        results
            .extend(search_mlx_models_with_query(&format!("mlx-community/{query}"), limit).await?);
        results.extend(search_mlx_models_with_query(&format!("google/{query}"), limit).await?);
    }
    dedupe_models(&mut results);
    results.truncate(limit);
    Ok(results)
}

async fn search_mlx_models_with_query(query: &str, limit: usize) -> Result<Vec<HfModelInfo>> {
    let client = hf_client().await?;
    let stream = client
        .list_models()
        .search(query.to_string())
        .sort("downloads".to_string())
        .limit(limit.saturating_mul(5).max(limit))
        .send()?;
    futures::pin_mut!(stream);

    let mut results = Vec::new();
    while let Some(info) = stream.next().await {
        let info = info?;
        if results.len() >= limit {
            break;
        }
        if let Some(model) = get_local_model_info_for_repo_with_client_and_downloads(
            &client,
            &info.id,
            info.downloads,
        )
        .await?
        {
            results.push(model);
        }
    }

    Ok(results)
}

async fn get_local_model_info_for_repo(repo_id: &str) -> Result<Option<HfModelInfo>> {
    let client = hf_client().await?;
    get_local_model_info_for_repo_with_client(&client, repo_id).await
}

async fn get_local_model_info_for_repo_with_client(
    client: &HFClient,
    repo_id: &str,
) -> Result<Option<HfModelInfo>> {
    get_local_model_info_for_repo_with_client_and_downloads(client, repo_id, None).await
}

async fn get_local_model_info_for_repo_with_client_and_downloads(
    client: &HFClient,
    repo_id: &str,
    downloads_hint: Option<u64>,
) -> Result<Option<HfModelInfo>> {
    let repo = model_repo(client, repo_id)?;
    let info = repo
        .info()
        .expand(vec![
            "siblings".to_string(),
            "config".to_string(),
            "safetensors".to_string(),
        ])
        .send()
        .await?;
    model_info_to_local_model_info(&repo, info, downloads_hint).await
}

async fn get_exact_name_local_model_info(model_name: &str) -> Result<Option<HfModelInfo>> {
    for owner in ["google", "mlx-community"] {
        let repo_id = format!("{owner}/{model_name}");
        if let Ok(Some(model)) = get_local_model_info_for_repo(&repo_id).await {
            return Ok(Some(model));
        }
    }
    Ok(None)
}

async fn model_info_to_local_model_info(
    repo: &HFRepository<RepoTypeModel>,
    info: ModelInfo,
    downloads_hint: Option<u64>,
) -> Result<Option<HfModelInfo>> {
    let repo_id = info.id.clone();
    let gguf_variants = get_repo_gguf_variants(&repo_id).await;
    let mut variants: Vec<HfModelVariant> = gguf_variants
        .as_deref()
        .unwrap_or_default()
        .iter()
        .map(|variant| variant.to_model_variant(&repo_id))
        .collect();
    if is_mlx_compatible_model_info(&info) {
        let mlx_config = load_repo_config_json(repo).await.unwrap_or_else(|error| {
            tracing::debug!(repo_id, %error, "Failed to load MLX config.json; falling back to API config");
            info.config.clone()
        });
        variants.extend(mlx_variants_from_model_info(&repo_id, &info, &mlx_config));
    }

    if variants.is_empty() {
        drop(gguf_variants?);

        let unusable: Vec<&str> = info
            .siblings
            .as_deref()
            .unwrap_or(&[])
            .iter()
            .map(|s| s.rfilename.as_str())
            .filter(|name| name.ends_with(".gguf"))
            .collect();
        if !unusable.is_empty() {
            tracing::warn!(
                repo_id,
                files = ?unusable,
                "Dropping repo from results: no GGUF file yielded a recognizable quantization"
            );
        }
        return Ok(None);
    }

    let author = info
        .author
        .unwrap_or_else(|| repo_id.split('/').next().unwrap_or_default().to_string());
    let model_name = repo_id
        .split('/')
        .next_back()
        .unwrap_or(&repo_id)
        .to_string();
    let downloads = match best_download_count(info.downloads, downloads_hint) {
        Some(downloads) => downloads,
        None => get_repo_downloads(&repo_id).await?.unwrap_or(0),
    };

    Ok(Some(HfModelInfo {
        repo_id,
        author,
        model_name,
        downloads,
        gguf_files: Vec::new(),
        variants,
    }))
}

fn best_download_count(primary: Option<u64>, hint: Option<u64>) -> Option<u64> {
    primary
        .filter(|downloads| *downloads > 0)
        .or_else(|| hint.filter(|downloads| *downloads > 0))
}

async fn get_repo_downloads(repo_id: &str) -> Result<Option<u64>> {
    let client = hf_client().await?;
    let repo = model_repo(&client, repo_id)?;
    Ok(repo.info().send().await?.downloads)
}

fn looks_like_repo_id(query: &str) -> bool {
    let Some((owner, repo)) = query.split_once('/') else {
        return false;
    };
    !owner.is_empty() && !repo.is_empty() && !repo.contains('/')
}

fn model_search_rank(query: &str, model: &HfModelInfo) -> u8 {
    let query = query.to_lowercase();
    let repo_id = model.repo_id.to_lowercase();
    let model_name = model.model_name.to_lowercase();

    if repo_id == query {
        0
    } else if model_name == query {
        1
    } else if repo_id.ends_with(&format!("/{query}")) {
        2
    } else if repo_id.contains(&query) {
        3
    } else {
        4
    }
}

fn dedupe_models(models: &mut Vec<HfModelInfo>) {
    let mut merged: Vec<HfModelInfo> = Vec::with_capacity(models.len());
    for model in std::mem::take(models) {
        if let Some(existing) = merged
            .iter_mut()
            .find(|existing| existing.repo_id == model.repo_id)
        {
            merge_model_info(existing, model);
        } else {
            merged.push(model);
        }
    }
    *models = merged;
}

fn merge_model_info(existing: &mut HfModelInfo, duplicate: HfModelInfo) {
    existing.downloads = existing.downloads.max(duplicate.downloads);

    let mut filenames: collections::HashSet<String> = existing
        .gguf_files
        .iter()
        .map(|file| file.filename.clone())
        .collect();
    existing.gguf_files.extend(
        duplicate
            .gguf_files
            .into_iter()
            .filter(|file| filenames.insert(file.filename.clone())),
    );

    let mut variant_keys: collections::HashSet<(String, String)> = existing
        .variants
        .iter()
        .map(|variant| (variant.backend_id.clone(), variant.variant_id.clone()))
        .collect();
    existing
        .variants
        .extend(duplicate.variants.into_iter().filter(|variant| {
            variant_keys.insert((variant.backend_id.clone(), variant.variant_id.clone()))
        }));
}

pub async fn get_repo_local_variants(repo_id: &str) -> Result<Vec<HfModelVariant>> {
    let mut variants: Vec<HfModelVariant> = get_repo_gguf_variants(repo_id)
        .await
        .unwrap_or_default()
        .iter()
        .map(|variant| variant.to_model_variant(repo_id))
        .collect();
    variants.extend(get_repo_mlx_variants(repo_id).await.unwrap_or_default());
    variants.sort_by(|a, b| {
        a.backend_id
            .cmp(&b.backend_id)
            .then_with(|| b.quality_rank.cmp(&a.quality_rank))
            .then_with(|| a.variant_id.cmp(&b.variant_id))
    });
    Ok(variants)
}

pub async fn resolve_local_model_selection(
    repo_id: &str,
    backend_id: &str,
    variant_id: Option<&str>,
) -> Result<ResolvedLocalModel> {
    match backend_id {
        MLX_BACKEND_ID => resolve_mlx_model(repo_id, variant_id.unwrap_or(MLX_VARIANT_ID)).await,
        LLAMACPP_BACKEND_ID => {
            let quantization = variant_id.ok_or_else(|| {
                anyhow::anyhow!("llama.cpp model '{}' is missing a quantization", repo_id)
            })?;
            resolve_gguf_model(repo_id, quantization).await
        }
        _ => bail!("Unknown local inference backend '{}'", backend_id),
    }
}

fn snapshot_root_for_file(path: &path::Path, repo_filename: &str) -> Option<path::PathBuf> {
    let mut root = path.to_path_buf();
    for _ in 0..repo_filename.split('/').count() {
        root.pop();
    }
    Some(root)
}

async fn resolve_gguf_model(repo_id: &str, quantization: &str) -> Result<ResolvedLocalModel> {
    let quantization = canonicalize_quantization(quantization);
    let spec = format!("{}:{}", repo_id, quantization);
    let (_repo, resolved) = resolve_model_spec_full(&spec).await?;
    let (local_paths, mmproj_path) =
        download_gguf_to_hf_cache(repo_id, &quantization, &resolved).await?;
    Ok(ResolvedLocalModel::Gguf {
        repo_id: repo_id.to_string(),
        quantization,
        resolved,
        local_paths,
        mmproj_path,
    })
}

async fn download_gguf_to_hf_cache(
    repo_id: &str,
    quantization: &str,
    resolved: &ResolvedModel,
) -> Result<(Vec<path::PathBuf>, Option<path::PathBuf>)> {
    let (owner, name) = split_repo_id(repo_id)?;
    let model_id = model_id_from_repo(repo_id, quantization);
    let total_size = resolved
        .files
        .iter()
        .chain(resolved.mmproj.iter())
        .map(|file| file.size_bytes)
        .sum();
    let progress = HfDownloadProgress::new(model_id, total_size);
    progress.init();
    let client = hf_client().await?;
    let repo = client.model(owner.to_string(), name.to_string());
    let mut paths = Vec::with_capacity(resolved.files.len());
    for file in &resolved.files {
        let path = match repo
            .download_file()
            .filename(file.filename.clone())
            .progress(progress.clone())
            .send()
            .await
            .map_err(anyhow::Error::from)
        {
            Ok(path) => path,
            Err(error) => {
                progress.fail(&error);
                return Err(error);
            }
        };
        progress.finish_file(file.size_bytes);
        paths.push(path);
    }

    let mmproj_path = if let Some(mmproj) = &resolved.mmproj {
        let path = match repo
            .download_file()
            .filename(mmproj.filename.clone())
            .progress(progress.clone())
            .send()
            .await
            .map_err(anyhow::Error::from)
        {
            Ok(path) => path,
            Err(error) => {
                progress.fail(&error);
                return Err(error);
            }
        };
        progress.finish_file(mmproj.size_bytes);
        Some(path)
    } else {
        None
    };

    progress.complete();
    Ok((paths, mmproj_path))
}

pub async fn resolve_local_model_spec(spec: &str) -> Result<ResolvedLocalModel> {
    match parse_model_spec(spec) {
        Ok((repo_id, quantization)) => return resolve_gguf_model(&repo_id, &quantization).await,
        Err(error) if spec.contains(':') => return Err(error),
        Err(_) => {}
    }

    if looks_like_repo_id(spec) {
        let variants = get_repo_local_variants(spec).await?;
        let mlx_variants: Vec<_> = variants
            .iter()
            .filter(|variant| variant.backend_id == MLX_BACKEND_ID)
            .collect();
        if mlx_variants.len() == 1
            && !variants
                .iter()
                .any(|variant| variant.backend_id == LLAMACPP_BACKEND_ID)
        {
            return resolve_mlx_model(spec, &mlx_variants[0].variant_id).await;
        }
        bail!(
            "Model spec '{}' is ambiguous; choose one of: {}",
            spec,
            variants
                .iter()
                .map(|variant| variant.download_id.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );
    }

    let (repo_id, quantization) = parse_model_spec(spec)?;
    resolve_gguf_model(&repo_id, &quantization).await
}

async fn resolve_mlx_model(repo_id: &str, variant_id: &str) -> Result<ResolvedLocalModel> {
    let variants = get_repo_mlx_variants(repo_id).await?;
    let Some(variant) = variants
        .iter()
        .find(|variant| variant.variant_id == variant_id)
    else {
        bail!("No MLX variant '{}' found in {}", variant_id, repo_id);
    };
    if !variant.supported {
        bail!(
            "MLX variant '{}' in {} is not supported: {}",
            variant_id,
            repo_id,
            variant
                .unsupported_reason
                .as_deref()
                .unwrap_or("unsupported by this build")
        );
    }
    let (owner, name) = split_repo_id(repo_id)?;
    let client = hf_client().await?;
    let repo = client.model(owner.to_string(), name.to_string());
    let info = repo
        .info()
        .expand(vec!["siblings".to_string(), "safetensors".to_string()])
        .send()
        .await?;
    let siblings = info.siblings.as_deref().unwrap_or(&[]);
    let filenames = mlx_download_filenames(siblings);
    let total_size = mlx_download_size_bytes(&info, siblings);
    let progress = HfDownloadProgress::new(repo_id.to_string(), total_size);
    progress.init();
    let mut snapshot_path = None;
    for filename in filenames {
        let file_size = siblings
            .iter()
            .find(|s| s.rfilename == filename)
            .and_then(|s| s.size)
            .unwrap_or(0);
        let path = match repo
            .download_file()
            .filename(filename.clone())
            .progress(progress.clone())
            .send()
            .await
            .map_err(anyhow::Error::from)
        {
            Ok(path) => path,
            Err(error) => {
                progress.fail(&error);
                return Err(error);
            }
        };
        if snapshot_path.is_none() {
            snapshot_path = snapshot_root_for_file(&path, &filename);
        }
        progress.finish_file(file_size);
    }
    progress.complete();
    let snapshot_path = snapshot_path
        .ok_or_else(|| anyhow::anyhow!("MLX model {} has no downloadable files", repo_id))?;
    let total_size = if total_size > 0 {
        total_size
    } else {
        dir_size(&snapshot_path)
    };
    Ok(ResolvedLocalModel::Mlx {
        repo_id: repo_id.to_string(),
        variant_id: variant_id.to_string(),
        snapshot_path,
        total_size,
    })
}

#[derive(Clone)]
struct HfDownloadProgress {
    model_id: String,
    total_bytes: u64,
    completed_bytes: Arc<Mutex<u64>>,
    state: Arc<Mutex<HfDownloadState>>,
}

#[derive(Default)]
struct HfDownloadState {
    bytes_downloaded: u64,
    current_file_total_bytes: u64,
    speed_bps: Option<u64>,
}

impl HfDownloadProgress {
    fn new(model_id: String, total_bytes: u64) -> Self {
        Self {
            model_id,
            total_bytes,
            completed_bytes: Arc::new(Mutex::new(0)),
            state: Arc::new(Mutex::new(HfDownloadState::default())),
        }
    }

    fn init(&self) {
        let manager = crate::download_manager::get_download_manager();
        let download_id = format!("{}-model", self.model_id);
        if manager.get_progress(&download_id).is_some() {
            manager.update_progress(&download_id, |progress| {
                if progress.status != download_manager::DownloadStatus::Cancelled {
                    progress.status = download_manager::DownloadStatus::Downloading;
                    progress.bytes_downloaded = 0;
                    progress.total_bytes = self.total_bytes;
                    progress.progress_percent = 0.0;
                    progress.speed_bps = None;
                    progress.eta_seconds = None;
                    progress.error = None;
                    progress.task_exited = false;
                }
            });
        } else {
            manager.set_progress(download_manager::DownloadProgress {
                model_id: download_id,
                status: download_manager::DownloadStatus::Downloading,
                bytes_downloaded: 0,
                total_bytes: self.total_bytes,
                progress_percent: 0.0,
                speed_bps: None,
                eta_seconds: None,
                error: None,
                task_exited: false,
            });
        }
    }

    fn is_cancelled(&self) -> bool {
        download_manager::get_download_manager()
            .get_progress(&format!("{}-model", self.model_id))
            .is_some_and(|progress| progress.status == download_manager::DownloadStatus::Cancelled)
    }

    fn complete(&self) {
        download_manager::get_download_manager().update_progress(
            &format!("{}-model", self.model_id),
            |progress| {
                if progress.status != download_manager::DownloadStatus::Cancelled {
                    progress.status = download_manager::DownloadStatus::Completed;
                    progress.progress_percent = 100.0;
                }
                progress.task_exited = true;
            },
        );
    }

    fn fail(&self, error: impl ToString) {
        download_manager::get_download_manager().update_progress(
            &format!("{}-model", self.model_id),
            |progress| {
                if progress.status != download_manager::DownloadStatus::Cancelled {
                    progress.status = download_manager::DownloadStatus::Failed;
                    progress.error = Some(error.to_string());
                }
                progress.task_exited = true;
            },
        );
    }

    fn finish_file(&self, size_bytes: u64) {
        let observed_size = self
            .state
            .lock()
            .map(|state| state.current_file_total_bytes.max(state.bytes_downloaded))
            .unwrap_or(0);
        if let Ok(mut completed_bytes) = self.completed_bytes.lock() {
            *completed_bytes = completed_bytes.saturating_add(size_bytes.max(observed_size));
        }
        if let Ok(mut state) = self.state.lock() {
            state.bytes_downloaded = 0;
            state.current_file_total_bytes = 0;
        }
        self.update_progress_from_state();
    }

    fn update_progress_from_state(&self) {
        let completed_bytes = self.completed_bytes.lock().map(|value| *value).unwrap_or(0);
        if let Ok(state) = self.state.lock() {
            let bytes_downloaded = completed_bytes.saturating_add(state.bytes_downloaded);
            let total_bytes =
                self.total_bytes
                    .max(completed_bytes.saturating_add(
                        state.current_file_total_bytes.max(state.bytes_downloaded),
                    ));
            update_download_manager_progress(
                &self.model_id,
                bytes_downloaded.min(total_bytes),
                total_bytes,
                state.speed_bps,
            );
        }
    }
}

impl ProgressHandler for HfDownloadProgress {
    fn on_progress(&self, event: &ProgressEvent) {
        let ProgressEvent::Download(event) = event else {
            return;
        };
        match event {
            DownloadEvent::Start { total_bytes, .. } => {
                if let Ok(mut state) = self.state.lock() {
                    state.current_file_total_bytes = *total_bytes;
                }
                self.update_progress_from_state();
            }
            DownloadEvent::Progress { files } => {
                if self.is_cancelled() {
                    return;
                }
                let bytes_downloaded = files
                    .iter()
                    .map(|file| {
                        if file.status == FileStatus::Complete {
                            file.total_bytes
                        } else {
                            file.bytes_completed
                        }
                    })
                    .sum();
                let total_bytes = files.iter().map(|file| file.total_bytes).sum();
                if let Ok(mut state) = self.state.lock() {
                    state.bytes_downloaded = state.bytes_downloaded.max(bytes_downloaded);
                    state.current_file_total_bytes =
                        state.current_file_total_bytes.max(total_bytes);
                }
                self.update_progress_from_state();
            }
            DownloadEvent::AggregateProgress {
                bytes_completed,
                total_bytes,
                bytes_per_sec,
            } => {
                if self.is_cancelled() {
                    return;
                }
                if let Ok(mut state) = self.state.lock() {
                    state.bytes_downloaded = state.bytes_downloaded.max(*bytes_completed);
                    state.current_file_total_bytes =
                        (*total_bytes).max(state.current_file_total_bytes);
                    state.speed_bps = bytes_per_sec.map(|speed| speed as u64);
                }
                self.update_progress_from_state();
            }
            DownloadEvent::Complete => {}
        }
    }
}

pub async fn cached_local_models() -> Result<Vec<CachedLocalModel>> {
    let client = hf_client().await?;
    let cache = client.scan_cache().send().await?;
    let mut models = collections::HashMap::new();

    for repo in cache
        .repos
        .into_iter()
        .filter(|repo| repo.repo_type == "model")
    {
        let mut revisions: Vec<_> = repo.revisions.iter().collect();
        revisions.sort_by_key(|revision| {
            (
                revision.refs.iter().any(|reference| reference == "main"),
                revision.last_modified,
            )
        });
        revisions.reverse();

        for revision in revisions {
            let siblings: Vec<HfApiSibling> = revision
                .files
                .iter()
                .map(|file| HfApiSibling {
                    rfilename: file.file_name.clone(),
                    size: Some(file.size_on_disk),
                })
                .collect();

            for variant in group_into_variants(&repo.repo_id, siblings.clone()).unwrap_or_default()
            {
                let mut matching: Vec<_> = revision
                    .files
                    .iter()
                    .filter(|file| {
                        file.file_name.ends_with(".gguf")
                            && !is_auxiliary_gguf_file(&file.file_name)
                            && parse_quantization(&file.file_name)
                                .eq_ignore_ascii_case(&variant.quantization)
                    })
                    .collect();
                matching.sort_by(|a, b| a.file_name.cmp(&b.file_name));

                let selected: Vec<_> = if variant.sharded {
                    let selected_shard_set = shard_set_key(&variant.filename);
                    matching
                        .into_iter()
                        .filter(|file| {
                            is_shard_file(&file.file_name)
                                && shard_set_key(&file.file_name) == selected_shard_set
                        })
                        .collect()
                } else {
                    matching
                        .into_iter()
                        .filter(|file| file.file_name == variant.filename)
                        .take(1)
                        .collect()
                };
                let Some(primary) = selected.first() else {
                    continue;
                };
                if variant.sharded
                    && parse_shard_total(&primary.file_name)
                        .is_none_or(|total| total as usize != selected.len())
                {
                    continue;
                }

                let mmproj = select_best_mmproj(
                    &repo.repo_id,
                    &siblings,
                    &primary.file_name,
                    &variant.quantization,
                )
                .and_then(|projector| {
                    revision
                        .files
                        .iter()
                        .find(|file| file.file_name == projector.filename)
                });

                let model = CachedLocalModel {
                    id: model_id_from_repo(&repo.repo_id, &variant.quantization),
                    repo_id: repo.repo_id.clone(),
                    filename: primary.file_name.clone(),
                    quantization: variant.quantization,
                    backend_id: LLAMACPP_BACKEND_ID.to_string(),
                    model_path: primary.file_path.clone(),
                    size_bytes: selected.iter().map(|file| file.size_on_disk).sum(),
                    mmproj_path: mmproj.map(|file| file.file_path.clone()),
                    mmproj_size_bytes: mmproj.map(|file| file.size_on_disk).unwrap_or(0),
                };
                models.entry(model.id.clone()).or_insert(model);
            }

            let config = revision
                .files
                .iter()
                .find(|file| file.file_name == "config.json")
                .and_then(|file| fs::read(&file.file_path).ok())
                .and_then(|contents| serde_json::from_slice(&contents).ok());
            let repo_siblings: Vec<RepoSibling> = siblings
                .iter()
                .map(|sibling| RepoSibling {
                    rfilename: sibling.rfilename.clone(),
                    size: sibling.size,
                    lfs: None,
                })
                .collect();
            let cached_filenames = revision
                .files
                .iter()
                .map(|file| file.file_name.as_str())
                .collect();
            let index_file = revision
                .files
                .iter()
                .find(|file| file.file_name == "model.safetensors.index.json");
            let index = index_file.and_then(|file| {
                std::fs::read(&file.file_path)
                    .ok()
                    .and_then(|contents| serde_json::from_slice(&contents).ok())
            });
            let valid_index = index_file.is_none() || index.is_some();
            if is_mlx_compatible_repo(&config, &repo_siblings)
                && is_mlx_runtime_supported(&config)
                && valid_index
                && mlx_snapshot_files_are_complete(&cached_filenames, index.as_ref())
            {
                let cached_files: Vec<_> = revision
                    .files
                    .iter()
                    .filter(|file| should_download_for_mlx(&file.file_name))
                    .collect();
                if !cached_files.is_empty() {
                    let model = CachedLocalModel {
                        id: repo.repo_id.clone(),
                        repo_id: repo.repo_id.clone(),
                        filename: MLX_VARIANT_ID.to_string(),
                        quantization: mlx_variant_id(&repo.repo_id, &config),
                        backend_id: MLX_BACKEND_ID.to_string(),
                        model_path: revision.snapshot_path.clone(),
                        size_bytes: cached_files.iter().map(|file| file.size_on_disk).sum(),
                        mmproj_path: None,
                        mmproj_size_bytes: 0,
                    };
                    models.entry(model.id.clone()).or_insert(model);
                }
            }
        }
    }

    let mut models: Vec<_> = models.into_values().collect();
    models.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(models)
}

pub async fn cached_local_model(model_id: &str) -> Result<Option<CachedLocalModel>> {
    Ok(cached_local_models()
        .await?
        .into_iter()
        .find(|model| model.id == model_id))
}

fn active_download_repo_id(progress: &download_manager::DownloadProgress) -> Option<&str> {
    let task_is_active = progress.status == download_manager::DownloadStatus::Downloading
        || (progress.status == download_manager::DownloadStatus::Cancelled
            && !progress.task_exited);
    if !task_is_active {
        return None;
    }

    let model_id = progress.model_id.strip_suffix("-model")?;
    Some(
        model_id
            .rsplit_once(':')
            .map_or(model_id, |(repo_id, _)| repo_id),
    )
}

pub async fn delete_cached_local_model(model_id: &str) -> Result<()> {
    let cached_models = cached_local_models().await?;
    let model = cached_models
        .iter()
        .find(|model| model.id == model_id)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("Model not found in the Hugging Face cache"))?;
    if download_manager::get_download_manager()
        .list_progress()
        .iter()
        .filter_map(active_download_repo_id)
        .any(|repo_id| repo_id == model.repo_id)
    {
        bail!(
            "Cannot delete '{}' while a model from repository '{}' is downloading",
            model.id,
            model.repo_id
        );
    }
    let client = hf_client().await?;
    let cache = client.scan_cache().send().await?;
    let other_models_in_repo = cached_models
        .iter()
        .any(|other| other.id != model.id && other.repo_id == model.repo_id);
    if !other_models_in_repo {
        let repo = cache
            .repos
            .iter()
            .find(|repo| repo.repo_type == "model" && repo.repo_id == model.repo_id)
            .ok_or_else(|| anyhow::anyhow!("Model cache repository not found"))?;
        fs::remove_dir_all(&repo.repo_path)?;
        return Ok(());
    }

    let mut blob_references = collections::HashMap::new();
    for file in cache
        .repos
        .iter()
        .flat_map(|repo| &repo.revisions)
        .flat_map(|revision| &revision.files)
    {
        *blob_references
            .entry(file.blob_path.clone())
            .or_insert(0usize) += 1;
    }

    let removable_mmproj_blob = model
        .mmproj_path
        .as_ref()
        .and_then(|path| std::fs::canonicalize(path).ok())
        .filter(|model_mmproj_blob| {
            !cached_models.iter().any(|other| {
                other.id != model.id
                    && other
                        .mmproj_path
                        .as_ref()
                        .and_then(|path| fs::canonicalize(path).ok())
                        .as_ref()
                        == Some(model_mmproj_blob)
            })
        });

    let paths: Vec<PathBuf> = cache
        .repos
        .iter()
        .filter(|repo| repo.repo_type == "model" && repo.repo_id == model.repo_id)
        .flat_map(|repo| &repo.revisions)
        .flat_map(|revision| &revision.files)
        .filter(|file| {
            if model.backend_id == MLX_BACKEND_ID {
                should_download_for_mlx(&file.file_name)
            } else {
                let is_model_weight = file.file_name.ends_with(".gguf")
                    && !is_auxiliary_gguf_file(&file.file_name)
                    && parse_quantization(&file.file_name)
                        .eq_ignore_ascii_case(&model.quantization);
                let is_unshared_mmproj = removable_mmproj_blob.as_ref() == Some(&file.blob_path);
                is_model_weight || is_unshared_mmproj
            }
        })
        .map(|file| file.file_path.clone())
        .collect();

    for path in &paths {
        let blob_path = fs::canonicalize(path).ok();
        fs::remove_file(path)?;
        if let Some(blob_path) = blob_path
            && let Some(references) = blob_references.get_mut(&blob_path)
        {
            *references -= 1;
            if *references == 0 {
                fs::remove_file(&blob_path)?;
            }
        }
    }

    Ok(())
}

fn update_download_manager_progress(
    model_id: &str,
    bytes_downloaded: u64,
    total_bytes: u64,
    speed_bps: Option<u64>,
) {
    download_manager::get_download_manager().update_progress(
        &format!("{}-model", model_id),
        |progress| {
            if progress.status == download_manager::DownloadStatus::Cancelled {
                return;
            }
            progress.bytes_downloaded = bytes_downloaded;
            progress.total_bytes = total_bytes;
            progress.progress_percent = if total_bytes > 0 {
                (bytes_downloaded as f64 / total_bytes as f64 * 100.0) as f32
            } else {
                0.0
            };
            progress.speed_bps = speed_bps;
        },
    );
}

fn dir_size(path: &path::Path) -> u64 {
    if path.is_file() {
        return fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    }
    let mut total = 0;
    if let Ok(entries) = fs::read_dir(path) {
        for entry in entries.flatten() {
            total += dir_size(&entry.path());
        }
    }
    total
}
