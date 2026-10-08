use crate::recipe::read_recipe_file_content::RecipeFile;
use crate::recipe::template_recipe::{
    ParsedRecipeTemplate, parse_recipe_content, parse_recipe_template, prepare_recipe_template,
};
use crate::recipe::value_deserializer::RecipeValueDeserializer;
use crate::recipe::{
    BUILT_IN_RECIPE_DIR_PARAM, Recipe, RecipeParameter, RecipeParameterInputType,
    RecipeParameterRequirement,
};
use anyhow::Result;
use serde_path_to_error::Segment;
use std::collections::{HashMap, HashSet};
use std::path::Path;
const MAX_PARAMETERS: usize = 32;
const MAX_TOTAL_SELECT_OPTIONS: usize = 200;
const MAX_PARAMETERS_SIZE_BYTES: usize = 128 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecipeFileFormat {
    Json,
    Yaml,
}

pub fn recipe_file_format(path: &Path) -> RecipeFileFormat {
    if path
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("json"))
    {
        RecipeFileFormat::Json
    } else {
        RecipeFileFormat::Yaml
    }
}

#[derive(Debug)]
pub enum SchedulerRecipeError {
    GenericParse(RecipeFileFormat),
    InvalidSchema(String),
}

impl std::fmt::Display for SchedulerRecipeError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SchedulerRecipeError::GenericParse(RecipeFileFormat::Json) => {
                write!(formatter, "Invalid JSON recipe")
            }
            SchedulerRecipeError::GenericParse(RecipeFileFormat::Yaml) => {
                write!(formatter, "Invalid YAML recipe")
            }
            SchedulerRecipeError::InvalidSchema(message) => {
                write!(formatter, "Invalid recipe: {message}")
            }
        }
    }
}

impl std::error::Error for SchedulerRecipeError {}

#[derive(Clone, Copy)]
enum SchemaNode {
    Recipe,
    Scalar,
    StringList,
    Settings,
    Author,
    ParameterList,
    Parameter,
    ExtensionList,
    Extension,
    Response,
    SubRecipeList,
    SubRecipe,
    Retry,
    CheckList,
    Check,
    Opaque,
}

struct SafeSchemaPath {
    display: String,
    node: SchemaNode,
    truncated: bool,
}

fn schema_field(node: SchemaNode, field: &str) -> Option<SchemaNode> {
    match node {
        SchemaNode::Recipe => match field {
            "version" | "title" | "description" | "instructions" | "prompt" => {
                Some(SchemaNode::Scalar)
            }
            "extensions" => Some(SchemaNode::ExtensionList),
            "settings" => Some(SchemaNode::Settings),
            "activities" => Some(SchemaNode::StringList),
            "author" => Some(SchemaNode::Author),
            "parameters" => Some(SchemaNode::ParameterList),
            "response" => Some(SchemaNode::Response),
            "sub_recipes" => Some(SchemaNode::SubRecipeList),
            "retry" => Some(SchemaNode::Retry),
            _ => None,
        },
        SchemaNode::Settings => match field {
            "bcaip_provider" | "bcaip_model" | "temperature" | "max_turns" => {
                Some(SchemaNode::Scalar)
            }
            _ => None,
        },
        SchemaNode::Author => match field {
            "contact" | "metadata" => Some(SchemaNode::Scalar),
            _ => None,
        },
        SchemaNode::Parameter => match field {
            "key" | "input_type" | "requirement" | "description" | "default" => {
                Some(SchemaNode::Scalar)
            }
            "options" => Some(SchemaNode::StringList),
            _ => None,
        },
        SchemaNode::Extension => match field {
            "type" | "name" | "description" | "cmd" | "timeout" | "cwd" | "bundled"
            | "display_name" | "uri" | "socket" | "client_id" | "client_secret_key"
            | "instructions" | "code" => Some(SchemaNode::Scalar),
            "args" | "env_keys" | "available_tools" | "scopes" | "dependencies" => {
                Some(SchemaNode::StringList)
            }
            "envs" | "headers" | "tools" => Some(SchemaNode::Opaque),
            _ => None,
        },
        SchemaNode::Response => match field {
            "json_schema" => Some(SchemaNode::Opaque),
            _ => None,
        },
        SchemaNode::SubRecipe => match field {
            "name" | "path" | "sequential_when_repeated" | "description" => {
                Some(SchemaNode::Scalar)
            }
            "values" => Some(SchemaNode::Opaque),
            _ => None,
        },
        SchemaNode::Retry => match field {
            "max_retries" | "on_failure" | "timeout_seconds" | "on_failure_timeout_seconds" => {
                Some(SchemaNode::Scalar)
            }
            "checks" => Some(SchemaNode::CheckList),
            _ => None,
        },
        SchemaNode::Check => match field {
            "type" | "command" => Some(SchemaNode::Scalar),
            _ => None,
        },
        _ => None,
    }
}

fn schema_sequence_item(node: SchemaNode) -> Option<SchemaNode> {
    match node {
        SchemaNode::StringList => Some(SchemaNode::Scalar),
        SchemaNode::ParameterList => Some(SchemaNode::Parameter),
        SchemaNode::ExtensionList => Some(SchemaNode::Extension),
        SchemaNode::SubRecipeList => Some(SchemaNode::SubRecipe),
        SchemaNode::CheckList => Some(SchemaNode::Check),
        _ => None,
    }
}

fn safe_schema_path(path: &serde_path_to_error::Path) -> Option<SafeSchemaPath> {
    let mut node = SchemaNode::Recipe;
    let mut display = String::new();

    for segment in path {
        if matches!(node, SchemaNode::Opaque) {
            return Some(SafeSchemaPath {
                display,
                node,
                truncated: true,
            });
        }

        match segment {
            Segment::Map { key } => {
                node = schema_field(node, key)?;
                if !display.is_empty() {
                    display.push('.');
                }
                display.push_str(key);
            }
            Segment::Seq { index } => {
                node = schema_sequence_item(node)?;
                display.push_str(&format!("[{index}]"));
            }
            Segment::Enum { .. } | Segment::Unknown => return None,
        }
    }

    Some(SafeSchemaPath {
        display,
        node,
        truncated: false,
    })
}

fn strip_path_prefix(message: &str, path: &str) -> String {
    if path.is_empty() {
        return message.to_string();
    }
    message
        .strip_prefix(path)
        .and_then(|rest| rest.strip_prefix(": "))
        .unwrap_or(message)
        .to_string()
}

fn schema_field_from_message<'a>(message: &'a str, prefix: &str) -> Option<&'a str> {
    message.strip_prefix(prefix)?.strip_suffix('`')
}

fn classify_conversion_error(
    error: &serde_path_to_error::Error<yaml_serde::Error>,
    format: RecipeFileFormat,
) -> SchedulerRecipeError {
    let Some(safe_path) = safe_schema_path(error.path()) else {
        return SchedulerRecipeError::GenericParse(format);
    };
    let raw_path = match error.path().to_string().as_str() {
        "." => String::new(),
        path => path.to_string(),
    };
    let message = strip_path_prefix(
        &crate::recipe::strip_error_location(&error.inner().to_string()),
        &raw_path,
    );

    if !safe_path.truncated {
        for (prefix, label) in [
            ("missing field `", "missing field"),
            ("duplicate field `", "duplicate field"),
        ] {
            if let Some(field) = schema_field_from_message(&message, prefix) {
                if schema_field(safe_path.node, field).is_some() {
                    let diagnostic = if safe_path.display.is_empty() {
                        format!("{label} `{field}`")
                    } else {
                        format!("{}: {label} `{field}`", safe_path.display)
                    };
                    return SchedulerRecipeError::InvalidSchema(diagnostic);
                }
            }
        }
    }

    if safe_path.display.is_empty() {
        SchedulerRecipeError::GenericParse(format)
    } else {
        SchedulerRecipeError::InvalidSchema(format!("{} is invalid", safe_path.display))
    }
}

fn convert_recipe_from_value(
    value: &yaml_serde::Value,
    format: RecipeFileFormat,
    coerce_scalars: bool,
) -> Result<Recipe, SchedulerRecipeError> {
    let result = if coerce_scalars {
        serde_path_to_error::deserialize(RecipeValueDeserializer::new(value))
    } else {
        serde_path_to_error::deserialize(value)
    };
    result.map_err(|error| classify_conversion_error(&error, format))
}

fn format_parameter_paths(indices: &[usize]) -> String {
    indices
        .iter()
        .map(|index| format!("parameters[{index}]"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn normalize_scheduling_parameter_key(key: &str) -> String {
    yaml_serde::from_str::<yaml_serde::Value>(key)
        .ok()
        .and_then(|value| match value {
            yaml_serde::Value::Bool(value) => Some(value.to_string()),
            yaml_serde::Value::Number(value) => Some(value.to_string()),
            yaml_serde::Value::Null => Some("null".to_string()),
            _ => None,
        })
        .unwrap_or_else(|| key.to_string())
}

fn validate_scheduling_parameters(
    parameters: &Option<Vec<RecipeParameter>>,
    template_variables: &HashSet<String>,
) -> Result<(), String> {
    let parameters = parameters.as_deref().unwrap_or_default();
    validate_parameter_limits(parameters).map_err(|e| e.to_string())?;

    let file_defaults = parameters
        .iter()
        .enumerate()
        .filter(|(_, parameter)| {
            matches!(parameter.input_type, RecipeParameterInputType::File)
                && parameter.default.is_some()
        })
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    if !file_defaults.is_empty() {
        return Err(format!(
            "file parameters cannot have default values at {}",
            format_parameter_paths(&file_defaults)
        ));
    }

    let optional_without_defaults = parameters
        .iter()
        .enumerate()
        .filter(|(_, parameter)| {
            matches!(parameter.requirement, RecipeParameterRequirement::Optional)
                && parameter.default.is_none()
        })
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    if !optional_without_defaults.is_empty() {
        return Err(format!(
            "optional parameters require default values at {}",
            format_parameter_paths(&optional_without_defaults)
        ));
    }

    let mut defined_keys = HashSet::new();
    for (index, parameter) in parameters.iter().enumerate() {
        if !defined_keys.insert(parameter.key.clone()) {
            return Err(format!(
                "duplicate parameter definition at parameters[{index}]"
            ));
        }
    }

    let mut referenced_keys = template_variables.clone();
    referenced_keys.remove(BUILT_IN_RECIPE_DIR_PARAM);
    let keys_match = |defined: &str, referenced: &str| {
        defined == referenced || defined == normalize_scheduling_parameter_key(referenced)
    };

    let missing_count = referenced_keys
        .iter()
        .filter(|referenced| {
            !defined_keys
                .iter()
                .any(|defined| keys_match(defined, referenced))
        })
        .count();
    if missing_count > 0 {
        return Err(format!(
            "missing parameter definitions for {missing_count} template variables"
        ));
    }

    let unnecessary = parameters
        .iter()
        .enumerate()
        .filter(|(_, parameter)| {
            !referenced_keys
                .iter()
                .any(|referenced| keys_match(&parameter.key, referenced))
        })
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    if !unnecessary.is_empty() {
        return Err(format!(
            "unnecessary parameter definitions at {}",
            format_parameter_paths(&unnecessary)
        ));
    }

    Ok(())
}

pub fn validate_recipe_for_scheduling(
    content: &str,
    recipe_dir: Option<String>,
    format: RecipeFileFormat,
) -> Result<Recipe, SchedulerRecipeError> {
    let (prepared_content, template_variables) = prepare_recipe_template(content, recipe_dir)
        .map_err(|_| SchedulerRecipeError::GenericParse(format))?;
    let document = yaml_serde::from_str::<yaml_serde::Value>(&prepared_content)
        .map_err(|_| SchedulerRecipeError::GenericParse(format))?;
    let nested_recipe = document.get("recipe");
    let recipe_value = nested_recipe.unwrap_or(&document);
    let mut recipe = convert_recipe_from_value(recipe_value, format, nested_recipe.is_none())?;
    recipe.ensure_analyze_for_developer();
    recipe.ensure_summon_for_subrecipes();

    validate_prompt_or_instructions(&recipe)
        .map_err(|error| SchedulerRecipeError::InvalidSchema(error.to_string()))?;
    validate_retry_config(&recipe)
        .map_err(|error| SchedulerRecipeError::InvalidSchema(error.to_string()))?;
    validate_scheduling_parameters(&recipe.parameters, &template_variables)
        .map_err(SchedulerRecipeError::InvalidSchema)?;
    if let Some(schema) = recipe
        .response
        .as_ref()
        .and_then(|response| response.json_schema.as_ref())
    {
        validate_json_schema(schema).map_err(|error| match error.to_string().as_str() {
            "JSON schema must be an object" | "Empty JSON schema is not allowed" => {
                SchedulerRecipeError::InvalidSchema(error.to_string())
            }
            _ => SchedulerRecipeError::InvalidSchema("response.json_schema is invalid".to_string()),
        })?;
    }

    Ok(recipe)
}

pub(crate) struct ValidatedRecipeTemplate {
    parsed: ParsedRecipeTemplate,
}

impl ValidatedRecipeTemplate {
    pub(crate) fn recipe(&self) -> &Recipe {
        self.parsed.recipe()
    }

    pub(crate) fn into_recipe(self) -> Recipe {
        self.parsed.into_recipe()
    }

    pub(crate) fn render(self, params: &HashMap<String, String>) -> Result<Recipe> {
        let (rendered_content, template_variables) = self.parsed.render(params)?;
        let recipe = Recipe::from_content(&rendered_content)?;
        validate_recipe_parameters(&recipe, &template_variables)?;
        validate_recipe_non_parameter_invariants(&recipe)?;
        Ok(recipe)
    }
}

pub fn parse_and_validate_parameters(
    recipe_file_content: &str,
    recipe_dir_str: Option<String>,
) -> Result<Recipe> {
    let (recipe_template, template_variables) =
        parse_recipe_content(recipe_file_content, recipe_dir_str)?;
    let recipe_parameters = &recipe_template.parameters;
    validate_optional_parameters(recipe_parameters)?;
    validate_parameters_in_template(recipe_parameters, &template_variables)?;
    Ok(recipe_template)
}

fn validate_recipe_parameters(recipe: &Recipe, template_variables: &HashSet<String>) -> Result<()> {
    validate_optional_parameters(&recipe.parameters)?;
    validate_parameters_in_template(&recipe.parameters, template_variables)
}

/// Drop parameter keys that no longer appear as `{{ }}` (session snapshots).
pub fn strip_unreferenced_parameters(mut recipe: Recipe) -> Recipe {
    if recipe
        .parameters
        .as_ref()
        .is_none_or(|parameters| parameters.is_empty())
    {
        recipe.parameters = None;
        return recipe;
    }

    let Ok(yaml) = recipe.to_yaml() else {
        return recipe;
    };
    let Ok((_, mut template_variables)) = parse_recipe_content(&yaml, None) else {
        return recipe;
    };
    template_variables.remove(BUILT_IN_RECIPE_DIR_PARAM);

    if let Some(parameters) = recipe.parameters.as_mut() {
        parameters.retain(|parameter| template_variables.contains(&parameter.key));
        if parameters.is_empty() {
            recipe.parameters = None;
        }
    }
    recipe
}

fn validate_json_schema(schema: &serde_json::Value) -> Result<()> {
    let schema_object = schema
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("JSON schema must be an object"))?;
    if schema_object.is_empty() {
        return Err(anyhow::anyhow!("Empty JSON schema is not allowed"));
    }
    jsonschema::validator_for(schema)
        .map(|_| ())
        .map_err(|error| anyhow::anyhow!("JSON schema validation failed: {error}"))
}

pub fn validate_recipe_template_from_file(recipe_file: &RecipeFile) -> Result<Recipe> {
    let recipe_dir = recipe_file
        .parent_dir
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("Error getting recipe directory"))?
        .to_string();

    validate_recipe_template_from_content(&recipe_file.content, Some(recipe_dir))
}

pub fn validate_recipe_template_from_content(
    recipe_content: &str,
    recipe_dir: Option<String>,
) -> Result<Recipe> {
    Ok(validate_recipe_template(recipe_content, recipe_dir)?.into_recipe())
}

pub(crate) fn validate_recipe_template(
    recipe_content: &str,
    recipe_dir: Option<String>,
) -> Result<ValidatedRecipeTemplate> {
    let parsed = parse_recipe_template(recipe_content, recipe_dir)?;
    validate_recipe_parameters(parsed.recipe(), parsed.template_variables())?;
    validate_recipe_non_parameter_invariants(parsed.recipe())?;
    Ok(ValidatedRecipeTemplate { parsed })
}

pub(crate) fn validate_recipe_non_parameter_invariants(recipe: &Recipe) -> Result<()> {
    validate_prompt_or_instructions(recipe)?;
    validate_retry_config(recipe)?;
    if let Some(response) = &recipe.response {
        if let Some(json_schema) = &response.json_schema {
            validate_json_schema(json_schema)?;
        }
    }
    Ok(())
}

fn validate_retry_config(recipe: &Recipe) -> Result<()> {
    if let Some(ref retry_config) = recipe.retry {
        if let Err(validation_error) = retry_config.validate() {
            return Err(anyhow::anyhow!(
                "Invalid retry configuration: {}",
                validation_error
            ));
        }
    }
    Ok(())
}

fn validate_prompt_or_instructions(recipe: &Recipe) -> Result<()> {
    let has_instructions = recipe
        .instructions
        .as_ref()
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false);
    let has_prompt = recipe
        .prompt
        .as_ref()
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false);

    if has_instructions || has_prompt {
        return Ok(());
    }

    Err(anyhow::anyhow!(
        "Recipe must specify at least one of `instructions` or `prompt`."
    ))
}

fn validate_parameters_in_template(
    recipe_parameters: &Option<Vec<RecipeParameter>>,
    template_variables: &HashSet<String>,
) -> Result<()> {
    let mut template_variables = template_variables.clone();
    template_variables.remove(BUILT_IN_RECIPE_DIR_PARAM);

    let mut param_keys = HashSet::new();
    for parameter in recipe_parameters.as_deref().unwrap_or_default() {
        if !param_keys.insert(parameter.key.clone()) {
            return Err(anyhow::anyhow!(
                "Duplicate parameter definition: {}.",
                parameter.key
            ));
        }
    }

    let missing_keys = template_variables
        .difference(&param_keys)
        .collect::<Vec<_>>();

    let extra_keys = param_keys
        .difference(&template_variables)
        .collect::<Vec<_>>();

    if missing_keys.is_empty() && extra_keys.is_empty() {
        return Ok(());
    }

    let mut message = String::new();

    if !missing_keys.is_empty() {
        message.push_str(&format!(
            "Missing definitions for parameters in the recipe file: {}.",
            missing_keys
                .iter()
                .map(|s| s.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }

    if !extra_keys.is_empty() {
        message.push_str(&format!(
            "\nUnnecessary parameter definitions: {}.",
            extra_keys
                .iter()
                .map(|s| s.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    Err(anyhow::anyhow!("{}", message.trim_end()))
}

fn validate_parameter_limits(parameters: &[RecipeParameter]) -> Result<()> {
    if parameters.len() > MAX_PARAMETERS {
        return Err(anyhow::anyhow!(
            "Recipe has {} parameters but the maximum is {}.",
            parameters.len(),
            MAX_PARAMETERS
        ));
    }

    let total_options: usize = parameters
        .iter()
        .map(|p| p.options.as_deref().unwrap_or_default().len())
        .sum();
    if total_options > MAX_TOTAL_SELECT_OPTIONS {
        return Err(anyhow::anyhow!(
            "Recipe has {} total select options but the maximum is {}.",
            total_options,
            MAX_TOTAL_SELECT_OPTIONS
        ));
    }

    let serialized = yaml_serde::to_string(&parameters)
        .map_err(|e| anyhow::anyhow!("Failed to serialize parameters: {}", e))?;
    if serialized.len() > MAX_PARAMETERS_SIZE_BYTES {
        return Err(anyhow::anyhow!(
            "Serialized parameters exceed the {} KiB limit.",
            MAX_PARAMETERS_SIZE_BYTES / 1024
        ));
    }

    Ok(())
}

fn validate_optional_parameters(parameters: &Option<Vec<RecipeParameter>>) -> Result<()> {
    let empty_params = vec![];
    let params = parameters.as_ref().unwrap_or(&empty_params);
    validate_parameter_limits(params)?;

    let file_params_with_defaults: Vec<String> = params
        .iter()
        .filter(|p| matches!(p.input_type, RecipeParameterInputType::File) && p.default.is_some())
        .map(|p| p.key.clone())
        .collect();

    if !file_params_with_defaults.is_empty() {
        return Err(anyhow::anyhow!(
            "File parameters cannot have default values to avoid importing sensitive user files: {}",
            file_params_with_defaults.join(", ")
        ));
    }

    let optional_params_without_default_values: Vec<String> = params
        .iter()
        .filter(|p| {
            matches!(p.requirement, RecipeParameterRequirement::Optional) && p.default.is_none()
        })
        .map(|p| p.key.clone())
        .collect();

    if optional_params_without_default_values.is_empty() {
        Ok(())
    } else {
        Err(anyhow::anyhow!(
            "Optional parameters missing default values in the recipe: {}. Please provide defaults.",
            optional_params_without_default_values.join(", ")
        ))
    }
}
