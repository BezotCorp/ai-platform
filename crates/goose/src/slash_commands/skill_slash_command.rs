use goose_sdk_types::custom_requests::{SourceEntry, SourceType};
use std::path::Path;

use crate::slash_commands::{SlashCommandEntry, SlashCommandSource};
use crate::{skills, slash_commands::util::normalize_command_name};

pub fn list_commands(working_dir: Option<&Path>) -> Vec<SlashCommandEntry> {
    commands_from_sources(skills::list_installed_skills(working_dir))
}

pub fn format_installed_skills(working_dir: Option<&Path>) -> String {
    let sources = skills::list_installed_skills(working_dir);
    let skills: Vec<_> = sources
        .iter()
        .filter(|s| matches!(s.source_type, SourceType::Skill | SourceType::BuiltinSkill))
        .collect();

    let mut output = String::new();
    if skills.is_empty() {
        output.push_str("No skills installed.\n\n");
        output.push_str("Skills are loaded from SKILL.md files in:\n");
        output.push_str("  - ~/.agents/skills/ (global)\n");
        output.push_str("  - ~/.agents/plugins/*/skills/ (installed plugins)\n");
        output.push_str("  - .agents/skills/ (in current project)\n");
    } else {
        output.push_str(&format!("**Installed skills ({}):**\n\n", skills.len()));
        for skill in &skills {
            let kind_label = if skill.source_type == SourceType::BuiltinSkill {
                " *(builtin)*"
            } else {
                ""
            };
            output.push_str(&format!(
                "- **{}**{}: {}\n",
                skill.name, kind_label, skill.description
            ));
        }
    }
    output
}

pub fn resolve_command(
    command: &str,
    params_str: &str,
    working_dir: Option<&Path>,
) -> Result<Option<String>, String> {
    let Some(skill) = skills::list_installed_skills(working_dir)
        .into_iter()
        .find(|skill| skill.name.eq_ignore_ascii_case(command))
    else {
        return Ok(None);
    };

    let args = (!params_str.is_empty()).then_some(params_str);
    let prompt = skills::loaded_skill_context_with_args(&skill, args)
        .map_err(|e| format!("Skill /{}: {}", command, e))?;

    Ok(Some(prompt))
}

pub(super) fn commands_from_sources(sources: Vec<SourceEntry>) -> Vec<SlashCommandEntry> {
    sources
        .into_iter()
        .filter_map(|source| {
            let name = normalize_command_name(&source.name);
            if name.is_empty() {
                return None;
            }
            let input_hint = skills::skill_argument_hint(&source);

            Some(SlashCommandEntry {
                name,
                description: source.description,
                source: SlashCommandSource::Skill,
                source_path: None,
                input_hint,
            })
        })
        .collect()
}
