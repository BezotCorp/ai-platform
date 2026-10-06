use anyhow::{Result, anyhow};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::{fs, hash::DefaultHasher};

use crate::recipe::{
    Recipe, build_recipe::resolve_sub_recipe_path, local_recipes::list_local_recipes,
};
#[derive(Debug, Clone)]
pub struct RecipeFileManifest {
    pub id: String,
    pub recipe: Recipe,
    pub file_path: PathBuf,
    pub last_modified: String,
}

pub fn short_id_from_path(path: &str) -> String {
    let mut hasher = DefaultHasher::new();
    path.hash(&mut hasher);
    let h = hasher.finish();
    format!("{:016x}", h)
}

pub fn list_recipe_file_manifests() -> Result<Vec<RecipeFileManifest>> {
    let recipes_with_path = list_local_recipes()?;
    let mut manifests = Vec::new();

    for (file_path, mut recipe) in recipes_with_path {
        let Ok(last_modified) = fs::metadata(file_path.clone()).and_then(|metadata| {
            metadata
                .modified()
                .map(|modified| chrono::DateTime::<chrono::Utc>::from(modified).to_rfc3339())
        }) else {
            continue;
        };

        resolve_recipe_sub_recipe_paths(&mut recipe, &file_path);

        manifests.push(RecipeFileManifest {
            id: short_id_from_path(file_path.to_string_lossy().as_ref()),
            recipe,
            file_path,
            last_modified,
        });
    }

    manifests.sort_by(|a, b| b.last_modified.cmp(&a.last_modified));

    Ok(manifests)
}

pub fn get_recipe_file_path_by_id(id: &str) -> Result<PathBuf> {
    list_recipe_file_manifests()?
        .into_iter()
        .find(|manifest| manifest.id == id)
        .map(|manifest| manifest.file_path)
        .ok_or_else(|| anyhow!("Recipe not found: {}", id))
}

pub fn load_recipe_by_id(id: &str) -> Result<Recipe> {
    let path = get_recipe_file_path_by_id(id)?;
    load_recipe_from_path(&path)
}

pub fn load_recipe_from_path(path: &Path) -> Result<Recipe> {
    let mut recipe = Recipe::from_file_path(path)?;
    resolve_recipe_sub_recipe_paths(&mut recipe, path);
    Ok(recipe)
}

fn resolve_recipe_sub_recipe_paths(recipe: &mut Recipe, recipe_path: &Path) {
    let Some(recipe_dir) = recipe_path.parent() else {
        return;
    };

    let Some(ref mut sub_recipes) = recipe.sub_recipes else {
        return;
    };

    for sub_recipe in sub_recipes.iter_mut() {
        if let Ok(resolved) = resolve_sub_recipe_path(&sub_recipe.path, recipe_dir) {
            sub_recipe.path = resolved;
        }
    }
}
