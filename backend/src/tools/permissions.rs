use std::path::{Component, Path};

use anyhow::{Result, bail};

use crate::tools::registry;

pub(crate) fn authorize_tool(name: &str) -> Result<()> {
    if !registry::contains(name) {
        bail!("Outil inconnu ou non autorisé");
    }
    Ok(())
}

pub(crate) fn authorize_path(path: &str) -> Result<()> {
    if path.is_empty() || path.len() > 1024 {
        bail!("Chemin invalide");
    }
    let relative = Path::new(path);
    if relative.is_absolute() {
        bail!("Les chemins absolus sont interdits");
    }
    for component in relative.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(name) => {
                let name = name.to_string_lossy();
                if matches!(
                    name.as_ref(),
                    ".git" | "target" | "node_modules" | ".ssh" | "secrets"
                ) || name.starts_with(".env")
                {
                    bail!("Chemin protégé");
                }
            }
            _ => {
                bail!("Traversée de répertoire interdite");
            }
        }
    }
    Ok(())
}
