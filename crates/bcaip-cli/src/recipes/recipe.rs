use crate::recipes::print_recipe::{
    missing_parameters_command_line, print_recipe_explanation,
    print_required_parameters_for_template,
};
use crate::recipes::search_recipe::load_recipe_file;
use anyhow::Result;
use bcaip::recipe::Recipe;
use bcaip::recipe::build_recipe::{
    RecipeError, apply_values_to_parameters_without_file_expansion, build_recipe_from_template,
};
use bcaip::recipe::validate_recipe::parse_and_validate_parameters;
fn create_user_prompt_callback() -> impl Fn(&str, &str) -> Result<String> {
    |key: &str, description: &str| -> Result<String> {
        let input_value =
            cliclack::input(format!("Please enter {} ({})", key, description)).interact()?;
        Ok(input_value)
    }
}

pub fn load_recipe(recipe_name: &str, params: Vec<(String, String)>) -> Result<Recipe> {
    let recipe_file = load_recipe_file(recipe_name)?;
    let recipe_content = recipe_file.content;
    let recipe_dir = recipe_file.parent_dir;
    match build_recipe_from_template(
        recipe_content,
        &recipe_dir,
        params,
        Some(create_user_prompt_callback()),
    ) {
        Ok(recipe) => Ok(recipe),
        Err(RecipeError::MissingParams { parameters }) => Err(anyhow::anyhow!(
            "Please provide the following parameters in the command line: {}",
            missing_parameters_command_line(parameters)
        )),
        Err(e) => Err(anyhow::anyhow!(e.to_string())),
    }
}

pub fn render_recipe_as_yaml(recipe_name: &str, params: Vec<(String, String)>) -> Result<()> {
    let recipe = load_recipe(recipe_name, params)?;
    match yaml_serde::to_string(&recipe) {
        Ok(yaml_content) => {
            println!("{}", yaml_content);
            Ok(())
        }
        Err(_) => {
            eprintln!("Failed to serialize recipe to YAML");
            std::process::exit(1);
        }
    }
}

pub fn explain_recipe(recipe_name: &str, params: Vec<(String, String)>) -> Result<()> {
    let recipe_file = load_recipe_file(recipe_name)?;
    let recipe_dir_str = recipe_file.parent_dir.display().to_string();
    let recipe_file_content = &recipe_file.content;
    let recipe_template =
        parse_and_validate_parameters(recipe_file_content, Some(recipe_dir_str.clone()))?;
    let recipe_parameters = recipe_template.parameters.clone();

    let (params_for_template, missing_params) = apply_values_to_parameters_without_file_expansion(
        &params,
        recipe_parameters,
        &recipe_dir_str,
        None::<fn(&str, &str) -> Result<String>>,
    )?;
    print_recipe_explanation(&recipe_template);
    print_required_parameters_for_template(params_for_template, missing_params);

    Ok(())
}
