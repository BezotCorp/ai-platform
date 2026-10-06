use crate::config::Config;
use crate::recipe::build_recipe::{RecipeError, build_recipe_from_template};
use crate::recipe::{Recipe, RecipeParameter, RecipeParameterRequirement};
use crate::slash_commands::util::normalize_command_name;
use crate::slash_commands::{SlashCommandEntry, SlashCommandSource};
use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, path::PathBuf};
use tracing::warn;

const SLASH_COMMANDS_CONFIG_KEY: &str = "slash_commands";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlashCommandMapping {
    pub command: String,
    pub recipe_path: String,
}

pub fn list_commands() -> Vec<SlashCommandMapping> {
    Config::global()
        .get_param(SLASH_COMMANDS_CONFIG_KEY)
        .unwrap_or_else(|err| {
            warn!(
                "Failed to load {}: {}. Falling back to empty list.",
                SLASH_COMMANDS_CONFIG_KEY, err
            );
            Vec::new()
        })
}

fn save_slash_commands(commands: Vec<SlashCommandMapping>) -> Result<()> {
    Config::global()
        .set_param(SLASH_COMMANDS_CONFIG_KEY, &commands)
        .map_err(|e| anyhow::anyhow!("Failed to save slash commands: {}", e))
}

pub fn set_recipe_slash_command(recipe_path: PathBuf, command: Option<String>) -> Result<()> {
    let recipe_path_str = recipe_path.to_string_lossy().to_string();

    let mut commands = list_commands();
    commands.retain(|mapping| mapping.recipe_path != recipe_path_str);

    if let Some(cmd) = command {
        let normalized_cmd = cmd.trim_start_matches('/').to_lowercase();
        if !normalized_cmd.is_empty() {
            commands.push(SlashCommandMapping {
                command: normalized_cmd,
                recipe_path: recipe_path_str,
            });
        }
    }

    save_slash_commands(commands)
}

pub fn get_recipe_for_command(command: &str) -> Option<PathBuf> {
    let normalized = command.trim_start_matches('/').to_lowercase();
    let commands = list_commands();
    commands
        .into_iter()
        .find(|mapping| mapping.command == normalized)
        .map(|mapping| PathBuf::from(mapping.recipe_path))
}

pub(super) fn commands_from_mappings(mappings: Vec<SlashCommandMapping>) -> Vec<SlashCommandEntry> {
    mappings
        .into_iter()
        .filter_map(|mapping| {
            let name = normalize_command_name(&mapping.command);
            if name.is_empty() {
                return None;
            }

            let metadata = recipe_entry(&mapping.recipe_path)?;

            Some(SlashCommandEntry {
                name,
                description: metadata.description,
                source: SlashCommandSource::Recipe,
                source_path: Some(mapping.recipe_path),
                input_hint: metadata.input_hint,
            })
        })
        .collect()
}

struct RecipeCommandMetadata {
    description: String,
    input_hint: Option<String>,
}

fn recipe_entry(recipe_path: &str) -> Option<RecipeCommandMetadata> {
    let recipe_path = PathBuf::from(recipe_path);
    if !recipe_path.exists() {
        return None;
    }

    let recipe_content = std::fs::read_to_string(&recipe_path).ok()?;
    let recipe_dir = recipe_path.parent()?;
    let recipe_dir_str = recipe_dir.display().to_string();
    let validation_result = crate::recipe::validate_recipe::validate_recipe_template_from_content(
        &recipe_content,
        Some(recipe_dir_str),
    )
    .ok()?;

    Some(RecipeCommandMetadata {
        description: validation_result.description,
        input_hint: input_hint_for_recipe(validation_result.parameters.as_ref()),
    })
}

fn input_hint_for_recipe(params: Option<&Vec<RecipeParameter>>) -> Option<String> {
    let params = params?;
    if params.is_empty() {
        return None;
    }

    let mut required = Vec::new();
    let mut optional = Vec::new();

    for p in params {
        match p.requirement {
            RecipeParameterRequirement::Required | RecipeParameterRequirement::UserPrompt => {
                required.push(format!("<{}>", p.key));
            }
            RecipeParameterRequirement::Optional => {
                optional.push(format!("[--{} <{}>]", p.key, p.key));
            }
        }
    }

    Some(
        required
            .into_iter()
            .chain(optional)
            .collect::<Vec<_>>()
            .join(" "),
    )
}

fn invalid_recipe_msg(command: &str, reason: impl std::fmt::Display) -> String {
    format!("Recipe /{} is not valid: {}", command, reason)
}

pub fn resolve_command(
    command: &str,
    params_str: &str,
) -> Result<Option<(Recipe, String)>, String> {
    let full_command = format!("/{}", command);
    let Some(recipe_path) = get_recipe_for_command(&full_command) else {
        return Ok(None);
    };

    if !recipe_path.exists() {
        return Ok(None);
    }

    let recipe_content =
        std::fs::read_to_string(&recipe_path).map_err(|e| invalid_recipe_msg(command, e))?;

    let recipe_dir = recipe_path
        .parent()
        .ok_or_else(|| invalid_recipe_msg(command, "unable to resolve recipe directory"))?;

    let recipe_dir_str = recipe_dir.display().to_string();
    let validation_result = crate::recipe::validate_recipe::validate_recipe_template_from_content(
        &recipe_content,
        Some(recipe_dir_str),
    )
    .map_err(|e| invalid_recipe_msg(command, e))?;

    let empty_params: Vec<RecipeParameter> = Vec::new();
    let all_params = validation_result
        .parameters
        .as_ref()
        .unwrap_or(&empty_params);
    let required: Vec<&RecipeParameter> = all_params
        .iter()
        .filter(|p| {
            matches!(
                p.requirement,
                RecipeParameterRequirement::Required | RecipeParameterRequirement::UserPrompt
            )
        })
        .collect();
    let optional: Vec<&RecipeParameter> = all_params
        .iter()
        .filter(|p| matches!(p.requirement, RecipeParameterRequirement::Optional))
        .collect();

    let param_values: Vec<(String, String)> = if params_str.is_empty() {
        vec![]
    } else if required.len() == 1 && optional.is_empty() {
        vec![(required[0].key.clone(), params_str.to_string())]
    } else {
        parse_recipe_args(params_str, &required, &optional)
            .map_err(|e| format!("Recipe /{}: {}", command, e))?
    };

    let recipe = build_recipe_from_template(
        recipe_content,
        recipe_dir,
        param_values,
        None::<fn(&str, &str) -> Result<String>>,
    )
    .map_err(|e| match e {
        RecipeError::MissingParams { parameters } => invalid_recipe_msg(
            command,
            format!("requires parameter(s): {}.", parameters.join(", "),),
        ),
        other => invalid_recipe_msg(command, other),
    })?;

    let prompt = [recipe.instructions.as_deref(), recipe.prompt.as_deref()]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join("\n\n");

    Ok(Some((recipe, prompt)))
}

fn parse_recipe_args(
    params_str: &str,
    required: &[&RecipeParameter],
    optional: &[&RecipeParameter],
) -> Result<Vec<(String, String)>> {
    let tokens = crate::utils::split_command_args(params_str)?;
    let required_keys: HashSet<&str> = required.iter().map(|p| p.key.as_str()).collect();
    let optional_keys: HashSet<&str> = optional.iter().map(|p| p.key.as_str()).collect();

    let mut positionals: Vec<String> = Vec::new();
    let mut flags: Vec<(String, String)> = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        let token = &tokens[i];
        if let Some(flag) = token.strip_prefix("--") {
            if required_keys.contains(flag) {
                return Err(anyhow!(
                    "Parameter '{}' is required; pass it positionally, not as --{}",
                    flag,
                    flag
                ));
            }
            if !optional_keys.contains(flag) {
                return Err(anyhow!("Unknown parameter: --{}", flag));
            }
            let value = tokens
                .get(i + 1)
                .filter(|v| !v.starts_with("--"))
                .ok_or_else(|| anyhow!("Missing value for --{}", flag))?;
            flags.push((flag.to_string(), value.clone()));
            i += 2;
        } else {
            positionals.push(token.clone());
            i += 1;
        }
    }

    let mut result = Vec::new();
    if required.len() == 1 && !positionals.is_empty() {
        result.push((required[0].key.clone(), positionals.join(" ")));
    } else {
        for (idx, value) in positionals.into_iter().enumerate() {
            if idx >= required.len() {
                return Err(anyhow!("Unexpected positional argument: {}", value));
            }
            result.push((required[idx].key.clone(), value));
        }
    }
    result.extend(flags);
    Ok(result)
}
