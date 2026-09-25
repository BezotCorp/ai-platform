use sha2::{Digest, Sha256};
use std::path;

pub(crate) fn checksum(content: &str) -> String {
    Sha256::digest(content.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

pub(crate) fn project_scope(path: &path::Path) -> String {
    checksum(&path.to_string_lossy())
}
