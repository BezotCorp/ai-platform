use crate::agents::ExtensionConfig;
use anyhow::Result;
use serde::Deserialize;
use std::path::Path;
#[derive(Debug, Deserialize)]
struct BundledExtensionEntry {
    id: String,
    name: String,
    #[serde(rename = "type")]
    extension_type: String,
    #[allow(dead_code)]
    #[serde(default)]
    enabled: bool,
}

pub fn validate_bundled_extensions(path: &Path) -> Result<String> {
    let content = std::fs::read_to_string(path)?;
    let raw_entries: Vec<serde_json::Value> = serde_json::from_str(&content)?;
    let total = raw_entries.len();
    let mut errors: Vec<String> = Vec::new();

    for (index, entry) in raw_entries.iter().enumerate() {
        let meta: BundledExtensionEntry = match serde_json::from_value(entry.clone()) {
            Ok(m) => m,
            Err(e) => {
                let id = entry
                    .get("id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                let name = entry
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                errors.push(format!(
                    "[{index}] {name} (id={id}): missing required metadata fields: {e}"
                ));
                continue;
            }
        };

        // Check for common field name mistakes before full deserialization
        if meta.extension_type == "streamable_http"
            && entry.get("url").is_some()
            && entry.get("uri").is_none()
        {
            errors.push(format!(
                "[{index}] {} (id={}): has \"url\" field but streamable_http expects \"uri\" — did you mean \"uri\"?",
                meta.name, meta.id
            ));
            continue;
        }

        if meta.extension_type == "stdio" && entry.get("cmd").is_none() {
            errors.push(format!(
                "[{index}] {} (id={}): stdio extension is missing required \"cmd\" field",
                meta.name, meta.id
            ));
            continue;
        }

        if let Err(e) = serde_json::from_value::<ExtensionConfig>(entry.clone()) {
            errors.push(format!("[{index}] {} (id={}): {e}", meta.name, meta.id));
        }
    }

    if errors.is_empty() {
        Ok(format!("✓ All {total} extensions validated successfully."))
    } else {
        let mut output = format!("✗ Found {} error(s) in {total} extensions:\n", errors.len());
        for error in &errors {
            output.push_str(&format!("\n  {error}"));
        }
        anyhow::bail!("{output}");
    }
}
