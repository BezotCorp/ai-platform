use anyhow::{Context, Result};
use bcaip::config::Config;
use bcaip_provider_types::conversations::{Conversation, Message};
use std::{
    fs,
    io::{Read, Write},
    path::Path,
    process::Command,
};
use tempfile::{Builder, NamedTempFile};
/// Resolve the editor command from config and environment variables.
/// Checks BCAIP_PROMPT_EDITOR, then $VISUAL, then $EDITOR.
pub fn resolve_editor_command() -> Option<String> {
    let config = Config::global();
    let config_editor = config.get_bcaip_prompt_editor().ok().flatten();
    let visual = std::env::var("VISUAL").ok();
    let editor_env = std::env::var("EDITOR").ok();
    resolve_editor_from_sources(
        config_editor.as_deref(),
        visual.as_deref(),
        editor_env.as_deref(),
    )
}

fn resolve_editor_from_sources(
    config_editor: Option<&str>,
    visual: Option<&str>,
    editor_env: Option<&str>,
) -> Option<String> {
    for cmd in [config_editor, visual, editor_env].into_iter().flatten() {
        if !cmd.is_empty() {
            return Some(cmd.to_string());
        }
    }
    None
}

/// Resolve the editor command, falling back to vi (or notepad on Windows).
pub fn resolve_editor_or_default() -> String {
    let config = Config::global();
    let config_editor = config.get_bcaip_prompt_editor().ok().flatten();
    let visual = std::env::var("VISUAL").ok();
    let editor_env = std::env::var("EDITOR").ok();
    resolve_editor_or_default_from_sources(
        config_editor.as_deref(),
        visual.as_deref(),
        editor_env.as_deref(),
    )
}

fn resolve_editor_default() -> String {
    if cfg!(windows) {
        "notepad".to_string()
    } else {
        "vi".to_string()
    }
}

fn resolve_editor_or_default_from_sources(
    config_editor: Option<&str>,
    visual: Option<&str>,
    editor_env: Option<&str>,
) -> String {
    resolve_editor_from_sources(config_editor, visual, editor_env)
        .unwrap_or_else(resolve_editor_default)
}

/// Open a YAML temp file with the user's editor to edit a conversation.
/// Returns the edited conversation, or an error if the editor failed or YAML was invalid.
pub fn edit_conversation(conversation: &Conversation) -> Result<Conversation> {
    let yaml = yaml_serde::to_string(conversation.messages())?;

    let mut tmp = NamedTempFile::with_suffix(".yaml")?;
    tmp.write_all(yaml.as_bytes())?;
    tmp.flush()?;

    let editor = resolve_editor_or_default();
    let path = tmp.path().to_path_buf();

    launch_editor(&editor, &path).with_context(|| format!("failed to launch editor '{editor}'"))?;

    let edited = std::fs::read_to_string(&path)?;
    let messages: Vec<Message> =
        yaml_serde::from_str(&edited).context("invalid YAML — session unchanged")?;

    Ok(Conversation::new_unvalidated(messages))
}

/// Build the markdown template content for the editor prompt.
fn build_template(messages: &[&str], prefill: Option<&str>) -> String {
    let mut content = String::from("# BCAIP Prompt Editor\n\n");

    content.push_str("# Your prompt:\n\n");
    if let Some(text) = prefill
        && !text.is_empty()
    {
        content.push_str(text);
        content.push('\n');
    }

    if !messages.is_empty() {
        content.push_str("# Recent conversation for context (newest first):\n\n");
        for message in messages.iter().rev() {
            content.push_str(&format!("{}\n", message));
        }
        content.push('\n');
    }

    content
}

/// Create temporary markdown file with conversation history and optional prefill text
fn create_temp_file(messages: &[&str], prefill: Option<&str>) -> Result<NamedTempFile> {
    let temp_file = Builder::new()
        .prefix("bcaip_prompt_")
        .suffix(".md")
        .tempfile()?;

    fs::write(temp_file.path(), build_template(messages, prefill))?;
    Ok(temp_file)
}

/// Split an editor command into program and arguments.
///
/// Uses shell-word splitting only when the command contains quotes, so values like
/// `"/Applications/Sublime Text.app/.../subl" -w` work. Unquoted commands are split on
/// whitespace to avoid shlex stripping backslashes from Windows paths like
/// `C:\Windows\System32\notepad.exe`.
fn split_editor_command(editor_cmd: &str) -> Result<Vec<String>> {
    if editor_cmd.contains(['"', '\'']) {
        shlex::split(editor_cmd).ok_or_else(|| {
            anyhow::anyhow!("Invalid editor command: unmatched quotes in '{editor_cmd}'")
        })
    } else {
        Ok(editor_cmd.split_whitespace().map(String::from).collect())
    }
}

/// Launch editor and wait for completion
fn launch_editor(editor_cmd: &str, file_path: &Path) -> Result<()> {
    use std::process::Stdio;
    let parts = split_editor_command(editor_cmd)?;
    if parts.is_empty() {
        return Err(anyhow::anyhow!("Empty editor command"));
    }

    let mut cmd = Command::new(&parts[0]);
    if let Ok(cwd) = std::env::current_dir() {
        cmd.current_dir(cwd);
    }
    if parts.len() > 1 {
        cmd.args(&parts[1..]);
    }
    cmd.arg(file_path)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());

    let status = cmd.status()?;

    if !status.success() {
        return Err(anyhow::anyhow!(
            "Editor exited with non-zero status: {}",
            status.code().unwrap_or(-1)
        ));
    }

    Ok(())
}

/// Main function to get input from editor
pub fn get_editor_input(
    editor_cmd: &str,
    messages: &[&str],
    prefill: Option<&str>,
) -> Result<(String, bool)> {
    let temp_file = create_temp_file(messages, prefill)?;
    let temp_path = temp_file.path().to_path_buf();

    launch_editor(editor_cmd, &temp_path)?;

    let mut content = String::new();
    let mut file = temp_file.reopen()?;
    file.read_to_string(&mut content)?;

    let user_input = extract_user_input(&content);

    let has_meaningful_content = !user_input.trim().is_empty();

    Ok((user_input, has_meaningful_content))
}

/// Extract only the user's input from the markdown file
fn extract_user_input(content: &str) -> String {
    if let Some(start) = content.find("# Your prompt:") {
        let marker_len = "# Your prompt:".len();
        #[allow(clippy::string_slice)]
        let user_section = &content[start + marker_len..];

        let end_patterns = [
            "# Recent conversation for context",
            "# Recent conversation for context (newest first):",
        ];

        let mut end_pos = None;
        for pattern in &end_patterns {
            if let Some(pos) = user_section.find(pattern) {
                end_pos = Some(pos);
                break;
            }
        }

        let user_input_section = match end_pos {
            Some(pos) =>
            {
                #[allow(clippy::string_slice)]
                &user_section[..pos]
            }
            None => user_section,
        };

        user_input_section.trim().to_string()
    } else {
        content.trim().to_string()
    }
}
