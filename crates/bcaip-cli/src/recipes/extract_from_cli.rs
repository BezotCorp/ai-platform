use anyhow::{Result, anyhow};
use goose::recipe::{Recipe, SubRecipe};
use std::path::PathBuf;

use crate::{
    cli::InputConfig,
    recipes::{
        print_recipe::print_recipe_info, recipe::load_recipe, search_recipe::load_recipe_file,
    },
};
pub fn extract_recipe_info_from_cli(
    recipe_name: String,
    params: Vec<(String, String)>,
    additional_sub_recipes: Vec<String>,
    quiet: bool,
) -> Result<(InputConfig, Recipe)> {
    let mut recipe = load_recipe(&recipe_name, params.clone()).unwrap_or_else(|err| {
        eprintln!("{}: {}", console::style("Error").red().bold(), err);
        std::process::exit(1);
    });
    if !quiet {
        print_recipe_info(&recipe, params);
    }

    if !additional_sub_recipes.is_empty() {
        let mut all_sub_recipes = recipe.sub_recipes.clone().unwrap_or_default();
        for sub_recipe_name in additional_sub_recipes {
            match load_recipe_file(&sub_recipe_name) {
                Ok(recipe_file) => {
                    let name = extract_recipe_name(&sub_recipe_name);
                    let recipe_file_path = recipe_file.file_path;
                    let additional_sub_recipe = SubRecipe {
                        path: recipe_file_path.to_string_lossy().to_string(),
                        name,
                        values: None,
                        sequential_when_repeated: true,
                        description: None,
                    };
                    all_sub_recipes.push(additional_sub_recipe);
                }
                Err(e) => {
                    return Err(anyhow!(
                        "Could not retrieve sub-recipe '{}': {}",
                        sub_recipe_name,
                        e
                    ));
                }
            }
        }
        recipe.sub_recipes = Some(all_sub_recipes);
    }

    let input_config = InputConfig {
        contents: recipe.prompt.clone().filter(|s| !s.trim().is_empty()),
        additional_system_prompt: recipe.instructions.clone(),
    };

    Ok((input_config, recipe))
}

fn extract_recipe_name(recipe_identifier: &str) -> String {
    // If it's a path (contains / or \), extract the file stem
    if recipe_identifier.contains('/') || recipe_identifier.contains('\\') {
        PathBuf::from(recipe_identifier)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string()
    } else {
        // If it's just a name (like "weekly-updates"), use it directly
        recipe_identifier.to_string()
    }
}
