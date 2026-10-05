use crate::recipes::github_recipe::RecipeSource;
use crate::recipes::search_recipe::{list_available_recipes, load_recipe_file};
use anyhow::Result;
use console::style;
use goose::recipe::validate_recipe::validate_recipe_template_from_file;
use goose::recipe_deeplink;
use std::collections::HashMap;
pub fn handle_validate(recipe_name: &str) -> Result<()> {
    // Load and validate the recipe file
    let recipe_file = load_recipe_file(recipe_name)?;
    validate_recipe_template_from_file(&recipe_file).map_err(|err| {
        anyhow::anyhow!(
            "{} recipe file is invalid: {}",
            style("✗").red().bold(),
            err
        )
    })?;
    println!("{} recipe file is valid", style("✓").green().bold());
    Ok(())
}

pub fn handle_deeplink(recipe_name: &str, params: &[String]) -> Result<String> {
    let params_map = parse_params(params)?;
    match generate_deeplink(recipe_name, params_map) {
        Ok((deeplink_url, recipe)) => {
            println!(
                "{} Generated deeplink for: {}",
                style("✓").green().bold(),
                recipe.title
            );
            println!("{}", deeplink_url);
            Ok(deeplink_url)
        }
        Err(err) => {
            println!(
                "{} Failed to encode recipe: {}",
                style("✗").red().bold(),
                err
            );
            Err(err)
        }
    }
}

pub fn handle_open(recipe_name: &str, params: &[String]) -> Result<()> {
    handle_open_with(
        recipe_name,
        params,
        |url| open::that(url),
        &mut std::io::stdout(),
    )
}

fn handle_open_with<F, W>(
    recipe_name: &str,
    params: &[String],
    opener: F,
    out: &mut W,
) -> Result<()>
where
    F: FnOnce(&str) -> std::io::Result<()>,
    W: std::io::Write,
{
    let params_map = parse_params(params)?;
    match generate_deeplink(recipe_name, params_map) {
        Ok((deeplink_url, recipe)) => match opener(&deeplink_url) {
            Ok(_) => {
                writeln!(
                    out,
                    "{} Opened recipe '{}' in Goose Desktop",
                    style("✓").green().bold(),
                    recipe.title
                )?;
                Ok(())
            }
            Err(err) => {
                writeln!(
                    out,
                    "{} Failed to open recipe in Goose Desktop: {}",
                    style("✗").red().bold(),
                    err
                )?;
                writeln!(out, "Generated deeplink: {}", deeplink_url)?;
                writeln!(
                    out,
                    "You can manually copy and open the URL above, or ensure Goose Desktop is installed."
                )?;
                Err(anyhow::anyhow!("Failed to open recipe: {}", err))
            }
        },
        Err(err) => {
            writeln!(
                out,
                "{} Failed to encode recipe: {}",
                style("✗").red().bold(),
                err
            )?;
            Err(err)
        }
    }
}

pub fn handle_list(format: &str, verbose: bool) -> Result<()> {
    let recipes = match list_available_recipes() {
        Ok(recipes) => recipes,
        Err(e) => {
            return Err(anyhow::anyhow!("Failed to list recipes: {}", e));
        }
    };

    match format {
        "json" => {
            println!("{}", serde_json::to_string(&recipes)?);
        }
        _ => {
            if recipes.is_empty() {
                println!("No recipes found");
                return Ok(());
            } else {
                println!("Available recipes:");
                for recipe in recipes {
                    let source_info = match recipe.source {
                        RecipeSource::Local => format!("local: {}", recipe.path),
                        RecipeSource::GitHub => format!("github: {}", recipe.path),
                    };

                    let description = if let Some(desc) = &recipe.description {
                        if desc.is_empty() { "(none)" } else { desc }
                    } else {
                        "(none)"
                    };

                    let output = format!("{} - {} - {}", recipe.name, description, source_info);
                    if verbose {
                        println!("  {}", output);
                        if let Some(title) = &recipe.title {
                            println!("    Title: {}", title);
                        }
                        println!("    Path: {}", recipe.path);
                    } else {
                        println!("{}", output);
                    }
                }
            }
        }
    }
    Ok(())
}

fn parse_params(params: &[String]) -> Result<HashMap<String, String>> {
    let mut params_map = HashMap::new();
    for param in params {
        let parts: Vec<&str> = param.splitn(2, '=').collect();
        if parts.len() != 2 {
            return Err(anyhow::anyhow!(
                "Invalid parameter format: '{}'. Expected format: key=value",
                param
            ));
        }
        params_map.insert(parts[0].to_string(), parts[1].to_string());
    }
    Ok(params_map)
}

fn generate_deeplink(
    recipe_name: &str,
    params: HashMap<String, String>,
) -> Result<(String, goose::recipe::Recipe)> {
    let recipe_file = load_recipe_file(recipe_name)?;
    // Load the recipe file first to validate it
    let recipe = validate_recipe_template_from_file(&recipe_file)?;
    match recipe_deeplink::encode(&recipe) {
        Ok(encoded) => {
            let mut full_url = format!("goose://recipe?config={}", encoded);

            // Append parameters as additional query parameters
            for (key, value) in params {
                // URL-encode the parameter keys and values
                let encoded_key = urlencoding::encode(&key);
                let encoded_value = urlencoding::encode(&value);
                full_url.push_str(&format!("&{}={}", encoded_key, encoded_value));
            }

            Ok((full_url, recipe))
        }
        Err(err) => Err(anyhow::anyhow!("Failed to encode recipe: {}", err)),
    }
}
