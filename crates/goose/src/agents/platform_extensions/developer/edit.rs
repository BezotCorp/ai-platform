use std::fs;
use std::path::{Path, PathBuf};

use rmcp::model::{Annotations, CallToolResult, ContentBlock, TextContent};
use schemars::JsonSchema;
use serde::Deserialize;
const NO_MATCH_PREVIEW_LINES: usize = 20;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct FileReadParams {
    /// Absolute path to the file to read.
    pub path: String,
    /// Line number to start reading from (1-based).
    #[schemars(range(min = 1))]
    pub line: Option<u32>,
    /// Maximum number of lines to read.
    pub limit: Option<u32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct FileWriteParams {
    pub path: String,
    pub content: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct FileEditParams {
    pub path: String,
    pub before: String,
    pub after: String,
}

pub struct EditTools;

fn visible_text(text: impl Into<String>) -> ContentBlock {
    ContentBlock::Text(
        TextContent::new(text).with_annotations(Annotations::default().with_priority(0.0)),
    )
}

impl EditTools {
    pub fn new() -> Self {
        Self
    }

    pub fn file_read_with_cwd(
        &self,
        params: FileReadParams,
        working_dir: Option<&Path>,
    ) -> CallToolResult {
        let path = resolve_path(&params.path, working_dir);

        match fs::read_to_string(&path) {
            Ok(content) => {
                let content = apply_line_limit(&content, params.line, params.limit);
                CallToolResult::success(vec![visible_text(content)])
            }
            Err(error) => CallToolResult::error(vec![visible_text(format!(
                "Failed to read {}: {}",
                params.path, error
            ))]),
        }
    }

    pub fn file_write(&self, params: FileWriteParams) -> CallToolResult {
        self.file_write_with_cwd(params, None)
    }

    pub fn file_write_with_cwd(
        &self,
        params: FileWriteParams,
        working_dir: Option<&Path>,
    ) -> CallToolResult {
        let path = resolve_path(&params.path, working_dir);

        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
            && !parent.exists()
            && let Err(error) = fs::create_dir_all(parent)
        {
            return CallToolResult::error(vec![visible_text(format!(
                "Failed to create directory {}: {}",
                parent.display(),
                error
            ))]);
        }

        let is_new = !path.exists();

        match fs::write(path, &params.content) {
            Ok(()) => {
                let line_count = params.content.lines().count();
                let action = if is_new { "Created" } else { "Wrote" };
                CallToolResult::success(vec![visible_text(format!(
                    "{} {} ({} lines)",
                    action, params.path, line_count
                ))])
            }
            Err(error) => CallToolResult::error(vec![visible_text(format!(
                "Failed to write {}: {}",
                params.path, error
            ))]),
        }
    }

    pub fn file_edit(&self, params: FileEditParams) -> CallToolResult {
        self.file_edit_with_cwd(params, None)
    }

    pub fn file_edit_with_cwd(
        &self,
        params: FileEditParams,
        working_dir: Option<&Path>,
    ) -> CallToolResult {
        let path = resolve_path(&params.path, working_dir);

        let content = match fs::read_to_string(&path) {
            Ok(c) => c,
            Err(error) => {
                return CallToolResult::error(vec![visible_text(format!(
                    "Failed to read {}: {}",
                    params.path, error
                ))]);
            }
        };

        let new_content = match string_replace(&content, &params.before, &params.after) {
            Ok(c) => c,
            Err(msg) => {
                return CallToolResult::error(vec![visible_text(msg)]);
            }
        };
        match fs::write(&path, &new_content) {
            Ok(()) => {
                let old_lines = params.before.lines().count();
                let new_lines = params.after.lines().count();
                CallToolResult::success(vec![visible_text(format!(
                    "Edited {} ({} lines -> {} lines)",
                    params.path, old_lines, new_lines
                ))])
            }
            Err(error) => CallToolResult::error(vec![visible_text(format!(
                "Failed to write {}: {}",
                params.path, error
            ))]),
        }
    }
}

impl Default for EditTools {
    fn default() -> Self {
        Self::new()
    }
}

pub fn string_replace(content: &str, before: &str, after: &str) -> Result<String, String> {
    let matches: Vec<_> = content.match_indices(before).collect();

    match matches.len() {
        0 => {
            let suggestion = find_similar_context(content, before);
            let mut msg = "No match found for the specified text.".to_string();
            if let Some(hint) = suggestion {
                msg.push_str(&format!("\n\nDid you mean:\n```\n{}\n```", hint));
            }
            let preview = build_file_preview(content, NO_MATCH_PREVIEW_LINES);
            msg.push_str(&format!("\n\nFile preview:\n```\n{}\n```", preview));
            Err(msg)
        }
        1 => Ok(content.replacen(before, after, 1)),
        n => {
            let mut msg = format!(
                "Found {} matches. Please provide more context to identify a unique match:\n",
                n
            );

            for (i, (pos, _)) in matches.iter().enumerate().take(2) {
                let line_num = count_lines_before(content, *pos);
                let context = get_line_context(content, line_num, 1);
                msg.push_str(&format!(
                    "\nMatch {} (line {}):\n```\n{}\n```",
                    i + 1,
                    line_num,
                    context
                ));
            }

            if n > 2 {
                msg.push_str(&format!("\n\n...and {} more", n - 2));
            }

            Err(msg)
        }
    }
}

fn apply_line_limit(content: &str, line: Option<u32>, limit: Option<u32>) -> String {
    if line.is_none() && limit.is_none() {
        return content.to_string();
    }
    let lines: Vec<&str> = content.split_inclusive('\n').collect();
    let start = line
        .map(|l| (l as usize).saturating_sub(1))
        .unwrap_or(0)
        .min(lines.len());
    let end = limit
        .map(|l| start + l as usize)
        .unwrap_or(lines.len())
        .min(lines.len());
    lines[start..end].concat()
}

pub fn resolve_path(path: &str, working_dir: Option<&Path>) -> PathBuf {
    let path = PathBuf::from(path);
    if path.is_absolute() {
        path
    } else {
        working_dir
            .map(Path::to_path_buf)
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| PathBuf::from("."))
            .join(path)
    }
}

fn count_lines_before(content: &str, byte_pos: usize) -> usize {
    content
        .char_indices()
        .take_while(|(i, _)| *i < byte_pos)
        .filter(|(_, c)| *c == '\n')
        .count()
        + 1
}

fn get_line_context(content: &str, target_line: usize, context: usize) -> String {
    let lines: Vec<&str> = content.lines().collect();
    let start = target_line.saturating_sub(context + 1);
    let end = (target_line + context).min(lines.len());

    lines[start..end].join("\n")
}

fn find_similar_context(content: &str, search: &str) -> Option<String> {
    let first_line = search.lines().next()?.trim();
    if first_line.is_empty() {
        return None;
    }

    for (i, line) in content.lines().enumerate() {
        if line.contains(first_line) || first_line.contains(line.trim()) {
            return Some(get_line_context(content, i + 1, 2));
        }
    }

    None
}

fn build_file_preview(content: &str, max_lines: usize) -> String {
    if content.is_empty() {
        return "(file is empty)".to_string();
    }

    let lines: Vec<&str> = content.lines().collect();
    let preview_end = lines.len().min(max_lines);
    let mut preview = lines[..preview_end]
        .iter()
        .enumerate()
        .map(|(index, line)| format!("{:>4}: {}", index + 1, line))
        .collect::<Vec<_>>()
        .join("\n");

    if lines.len() > preview_end {
        preview.push_str(&format!("\n... ({} more lines)", lines.len() - preview_end));
    }

    preview
}
