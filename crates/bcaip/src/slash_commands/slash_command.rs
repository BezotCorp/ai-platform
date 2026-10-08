use crate::slash_commands::util::normalize_command_name;
use crate::slash_commands::{SlashCommandEntry, SlashCommandSource};
use std::{collections::HashSet, path::Path};

pub fn list_builtin_commands() -> Vec<SlashCommandEntry> {
    crate::agents::execute_commands::list_commands()
        .iter()
        .map(|command| SlashCommandEntry {
            name: command.name.to_string(),
            description: command.description.to_string(),
            source: SlashCommandSource::Builtin,
            source_path: None,
            input_hint: None,
        })
        .collect()
}

pub fn list_acp_commands(working_dir: Option<&Path>) -> Vec<SlashCommandEntry> {
    merge_command_sources(
        list_builtin_commands(),
        super::recipe_slash_command::commands_from_mappings(
            super::recipe_slash_command::list_commands(),
        ),
        super::skill_slash_command::list_commands(working_dir),
    )
}

pub(super) fn merge_command_sources(
    builtins: Vec<SlashCommandEntry>,
    recipes: Vec<SlashCommandEntry>,
    skills: Vec<SlashCommandEntry>,
) -> Vec<SlashCommandEntry> {
    let mut commands = builtins;
    let mut reserved_names: HashSet<String> = commands
        .iter()
        .map(|command| normalize_command_name(&command.name))
        .collect();

    for command in recipes {
        if reserved_names.insert(normalize_command_name(&command.name)) {
            commands.push(command);
        }
    }

    commands.extend(
        skills
            .into_iter()
            .filter(|command| !reserved_names.contains(&normalize_command_name(&command.name))),
    );
    commands
}
