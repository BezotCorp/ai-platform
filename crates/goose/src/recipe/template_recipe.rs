use std::{
    collections::{HashMap, HashSet},
    path::{Component, Path},
};

use crate::recipe::{BUILT_IN_RECIPE_DIR_PARAM, Recipe};
use anyhow::Result;
use minijinja::{Environment, UndefinedBehavior};
use regex::Regex;
const CURRENT_TEMPLATE_NAME: &str = "recipe";
const OPEN_BRACE: &str = "{{";
const CLOSE_BRACE: &str = "}}";

pub(crate) struct ParsedRecipeTemplate {
    recipe: Recipe,
    template_variables: HashSet<String>,
    environment: Environment<'static>,
}

impl ParsedRecipeTemplate {
    pub(crate) fn recipe(&self) -> &Recipe {
        &self.recipe
    }

    pub(crate) fn template_variables(&self) -> &HashSet<String> {
        &self.template_variables
    }

    pub(crate) fn into_recipe(self) -> Recipe {
        self.recipe
    }

    pub(crate) fn render(
        mut self,
        params: &HashMap<String, String>,
    ) -> Result<(String, HashSet<String>)> {
        self.environment
            .set_undefined_behavior(UndefinedBehavior::Strict);
        self.environment.set_loader(|_| Ok(None));
        let template = self.environment.get_template(CURRENT_TEMPLATE_NAME)?;
        let rendered = template
            .render(params)
            .map_err(|error| anyhow::anyhow!("Failed to render the recipe {error}"))?;
        Ok((rendered, self.template_variables))
    }
}

fn preprocess_template_variables(content: &str) -> Result<String> {
    let all_template_variables = extract_template_variables(content);
    let complex_template_variables = filter_complex_variables(&all_template_variables);
    let unparsable_template_variables = filter_unparseable_variables(&complex_template_variables)?;
    replace_unparseable_vars_with_raw(content, &unparsable_template_variables)
}

fn extract_template_variables(content: &str) -> Vec<String> {
    let template_var_re = Regex::new(r"\{\{(.*?)\}\}").unwrap();
    template_var_re
        .captures_iter(content)
        .map(|cap| cap[1].to_string())
        .collect()
}

// filter out variables that are not only alphanumeric and underscores
fn filter_complex_variables(template_variables: &[String]) -> Vec<String> {
    let valid_var_re = Regex::new(r"^\s*[a-zA-Z_][a-zA-Z0-9_]*\s*$").unwrap();
    template_variables
        .iter()
        .filter(|var| !valid_var_re.is_match(var))
        .cloned()
        .collect()
}

fn filter_unparseable_variables(template_variables: &[String]) -> Result<Vec<String>> {
    let mut vars_to_convert = Vec::new();

    for var in template_variables {
        let trimmed = var.trim();

        if trimmed.starts_with('\'') || trimmed.starts_with('"') {
            continue;
        }

        let mut env = Environment::new();
        env.set_undefined_behavior(UndefinedBehavior::Lenient);

        let test_template = format!(
            "{open}{content}{close}",
            open = OPEN_BRACE,
            content = var,
            close = CLOSE_BRACE
        );
        if env.template_from_str(&test_template).is_err() {
            vars_to_convert.push(var.clone());
        }
    }

    Ok(vars_to_convert)
}

fn replace_unparseable_vars_with_raw(
    content: &str,
    unparsable_template_variables: &[String],
) -> Result<String> {
    let mut result = content.to_string();

    for var in unparsable_template_variables {
        let pattern = format!(
            "{open}{content}{close}",
            open = OPEN_BRACE,
            content = var,
            close = CLOSE_BRACE
        );
        let replacement = format!(
            "{{% raw %}}{open}{content}{close}{{% endraw %}}",
            open = OPEN_BRACE,
            close = CLOSE_BRACE,
            content = var
        );
        result = result.replace(&pattern, &replacement);
    }

    Ok(result)
}

pub fn render_recipe_content_with_params(
    content: &str,
    params: &HashMap<String, String>,
) -> Result<String> {
    let empty_quotes = Regex::new(r#":\s*"""#).unwrap();
    let content_with_empty_quotes_replaced = empty_quotes.replace_all(content, ": ''");
    let content_with_safe_variables =
        preprocess_template_variables(&content_with_empty_quotes_replaced)?;

    let env = add_template_in_env(
        &content_with_safe_variables,
        params.get(BUILT_IN_RECIPE_DIR_PARAM).cloned(),
        UndefinedBehavior::Strict,
    )?;
    let template = env.get_template(CURRENT_TEMPLATE_NAME).unwrap();
    let rendered_content = template
        .render(params)
        .map_err(|e| anyhow::anyhow!("Failed to render the recipe {}", e))?;
    Ok(rendered_content)
}

fn add_template_in_env(
    content: &str,
    recipe_dir: Option<String>,
    undefined_behavior: UndefinedBehavior,
) -> Result<Environment<'static>> {
    let mut env = minijinja::Environment::new();
    env.set_undefined_behavior(undefined_behavior);

    if let Some(recipe_dir) = recipe_dir {
        env.set_loader(move |name| {
            load_template_from_recipe_dir(Path::new(recipe_dir.as_str()), name)
        });
    }

    env.add_template_owned(CURRENT_TEMPLATE_NAME, content.to_string())?;
    Ok(env)
}

fn load_template_from_recipe_dir(
    recipe_dir: &Path,
    name: &str,
) -> Result<Option<String>, minijinja::Error> {
    if Path::new(name).components().any(|component| {
        matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) {
        return Err(minijinja::Error::new(
            minijinja::ErrorKind::InvalidOperation,
            "template path must stay within the recipe directory",
        ));
    }

    let recipe_dir = match std::fs::canonicalize(recipe_dir) {
        Ok(path) => path,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(template_loader_error(error)),
    };
    let path = match std::fs::canonicalize(recipe_dir.join(name)) {
        Ok(path) => path,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(template_loader_error(error)),
    };

    if !path.starts_with(&recipe_dir) {
        return Err(minijinja::Error::new(
            minijinja::ErrorKind::InvalidOperation,
            "template path must stay within the recipe directory",
        ));
    }

    match std::fs::read_to_string(path) {
        Ok(content) => Ok(Some(content)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(template_loader_error(error)),
    }
}

fn template_loader_error(error: std::io::Error) -> minijinja::Error {
    minijinja::Error::new(
        minijinja::ErrorKind::InvalidOperation,
        "could not read template",
    )
    .with_source(error)
}

fn get_env_with_template_variables(
    content: &str,
    recipe_dir: Option<String>,
    undefined_behavior: UndefinedBehavior,
) -> Result<(Environment<'static>, HashSet<String>)> {
    let env = add_template_in_env(content, recipe_dir, undefined_behavior)?;
    let template_variables = {
        let template = env.get_template(CURRENT_TEMPLATE_NAME).unwrap();
        let captured = template.render_captured(())?;
        let state = captured.state();
        let mut vars = HashSet::new();
        for (_, tmpl) in state.env().templates() {
            vars.extend(tmpl.undeclared_variables(true));
        }
        vars
    };
    Ok((env, template_variables))
}

fn uses_template_inheritance(content: &str) -> bool {
    let re = Regex::new(r"\{%-?\s*(extends|include)").unwrap();
    re.is_match(content)
}

pub fn parse_recipe_content(
    content: &str,
    recipe_dir: Option<String>,
) -> Result<(Recipe, HashSet<String>)> {
    let parsed = parse_recipe_template(content, recipe_dir)?;
    Ok((parsed.recipe, parsed.template_variables))
}

fn prepare_recipe_template_with_environment(
    content: &str,
    recipe_dir: Option<String>,
) -> Result<(String, HashSet<String>, Environment<'static>)> {
    let preprocessed_content = preprocess_template_variables(content)?;
    let (env, template_variables) = get_env_with_template_variables(
        &preprocessed_content,
        recipe_dir,
        UndefinedBehavior::Lenient,
    )?;
    let template = env.get_template(CURRENT_TEMPLATE_NAME).unwrap();
    let recipe_content = if uses_template_inheritance(&preprocessed_content) {
        template
            .render(())
            .map_err(|e| anyhow::anyhow!("Failed to parse the recipe {}", e))?
    } else {
        preprocessed_content
    };
    Ok((recipe_content, template_variables, env))
}

pub(crate) fn prepare_recipe_template(
    content: &str,
    recipe_dir: Option<String>,
) -> Result<(String, HashSet<String>)> {
    let (recipe_content, template_variables, _) =
        prepare_recipe_template_with_environment(content, recipe_dir)?;
    Ok((recipe_content, template_variables))
}

pub(crate) fn parse_recipe_template(
    content: &str,
    recipe_dir: Option<String>,
) -> Result<ParsedRecipeTemplate> {
    let (recipe_content, template_variables, environment) =
        prepare_recipe_template_with_environment(content, recipe_dir)?;
    let recipe = Recipe::from_content(&recipe_content)?;
    Ok(ParsedRecipeTemplate {
        recipe,
        template_variables,
        environment,
    })
}
