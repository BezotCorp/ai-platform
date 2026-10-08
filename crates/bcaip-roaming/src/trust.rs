//! Local access-control state: which peer keys this node accepts inbound
//! connections from, and which are revoked.
//!
//! Trust is a **mutual, public-key allowlist**. A peer is identified by the key
//! iroh's QUIC-TLS handshake authenticated, and is admitted only if that key is
//! on this node's allowlist. There is no bearer/token mode: sharing a
//! [`crate::ConnectionCard`] grants nothing until the recipient explicitly
//! accepts the sender's key. An accepted peer gets BCAIP's full ACP surface.
//!
//! This is deliberately local, unsigned admin state: it lives on the host under
//! the user's control. Authentication of *who* a peer is comes from the
//! transport; this layer decides *whether* they are authorized.

use iroh::EndpointId;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Persisted trust state: the inbound allowlist plus revocations.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TrustBook {
    /// Peer keys allowed to connect.
    allowed: BTreeSet<String>,
    /// Peer keys that are refused regardless of anything else.
    revoked_keys: BTreeSet<String>,
}

impl TrustBook {
    pub fn new() -> Self {
        Self::default()
    }

    /// Accept inbound connections from `key`. Clears any prior revocation.
    pub fn accept(&mut self, key: &EndpointId) {
        let s = key_str(key);
        self.revoked_keys.remove(&s);
        self.allowed.insert(s);
    }

    /// Stop accepting `key` and record it as revoked so a stale card can't
    /// silently re-add it.
    pub fn revoke_key(&mut self, key: &EndpointId) {
        let s = key_str(key);
        self.allowed.remove(&s);
        self.revoked_keys.insert(s);
    }

    /// Whether `key` is allowed to connect (on the allowlist and not revoked).
    pub fn is_allowed(&self, key: &EndpointId) -> bool {
        let s = key_str(key);
        !self.revoked_keys.contains(&s) && self.allowed.contains(&s)
    }

    pub(crate) fn is_key_revoked(&self, key: &EndpointId) -> bool {
        self.revoked_keys.contains(&key_str(key))
    }

    /// Allowed peer keys, sorted.
    pub fn allowed_keys(&self) -> Vec<String> {
        self.allowed.iter().cloned().collect()
    }

    /// Load the trust book. A missing file is an empty book; a *malformed*
    /// file is a hard error — silently treating corruption as "no one is
    /// allowed" would strand peers, and treating it as "keep going" would be
    /// worse. Callers on the authorization path fail closed on this error.
    pub fn load(path: &std::path::Path) -> Result<Self, std::io::Error> {
        match std::fs::read(path) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(std::io::Error::other),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e),
        }
    }

    /// Persist atomically (unique temp file + rename) so a concurrent reader
    /// on the authorization path never observes a half-written file, and
    /// concurrent writers never truncate each other's in-flight temp file.
    pub fn save(&self, path: &std::path::Path) -> Result<(), std::io::Error> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?;
        let tmp = path.with_extension(format!("json.tmp-{}", std::process::id()));
        std::fs::write(&tmp, json)?;
        std::fs::rename(&tmp, path)
    }

    /// Read-modify-write the trust book under a cross-process advisory lock.
    ///
    /// Atomic replacement in [`save`] protects readers from partial JSON but
    /// not writers from lost updates: two `bcaip roam peers` commands (or any
    /// other embedder) each load the whole book, mutate, and save, so the last
    /// writer clobbers the other's change with a stale snapshot —
    /// e.g. a concurrent accept resurrects a peer that was just revoked. This
    /// serializes the whole load+mutate+save so those edits can't race. The
    /// lock is held on a sidecar `.lock` file (never the book itself) and
    /// auto-releases if the holder dies.
    pub fn update(
        path: &std::path::Path,
        mutate: impl FnOnce(&mut Self),
    ) -> Result<Self, std::io::Error> {
        use fs2::FileExt as _;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let lock_path = path.with_extension("json.lock");
        let lock = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)?;
        lock.lock_exclusive()?;

        let result = (|| {
            let mut book = Self::load(path)?;
            mutate(&mut book);
            book.save(path)?;
            Ok(book)
        })();

        let _ = fs2::FileExt::unlock(&lock);
        result
    }
}

fn key_str(key: &EndpointId) -> String {
    key.to_string()
}
