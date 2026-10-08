use crate::agents::extension::PlatformExtensionContext;
use crate::agents::mcp_client::{Error, McpClientTrait};
use crate::agents::tool_execution::ToolCallContext;
use async_trait::async_trait;
use bcaip_provider_types::base::Provider;
use bcaip_provider_types::conversations::Message;
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use rmcp::model::{
    CallToolResult, ContentBlock, Implementation, InitializeResult, JsonObject, ListToolsResult,
    ServerCapabilities, Tool,
};
use schemars::{JsonSchema, schema_for};
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
pub static EXTENSION_NAME: &str = "summarize";

const MAX_FILE_SIZE: u64 = 100 * 1024;
const MAX_TOTAL_SIZE: usize = 1024 * 1024;

#[derive(Debug, Deserialize, JsonSchema)]
struct SummarizeParams {
    /// Files or directories to include. Directories are expanded recursively.
    paths: Vec<String>,
    /// What to focus on or ask about the content. This guides the summary.
    question: String,
    /// File extensions to include (e.g., ["rs", "py"]). If not specified, includes all files.
    extensions: Option<Vec<String>>,
}

pub struct SummarizeClient {
    info: InitializeResult,
    context: PlatformExtensionContext,
}

impl SummarizeClient {
    pub fn new(context: PlatformExtensionContext) -> anyhow::Result<Self> {
        let info = InitializeResult::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(
                Implementation::new(EXTENSION_NAME.to_string(), "1.0.0".to_string())
                    .with_title("Summarize"),
            );

        Ok(Self { info, context })
    }

    async fn get_provider(&self) -> Result<Arc<dyn Provider>, String> {
        let extension_manager = self
            .context
            .extension_manager
            .as_ref()
            .and_then(|weak| weak.upgrade())
            .ok_or("Extension manager not available")?;

        let provider_guard = extension_manager.get_provider().lock().await;

        let provider = provider_guard
            .as_ref()
            .ok_or("Provider not available")?
            .clone();

        Ok(provider)
    }

    fn get_tools() -> Vec<Tool> {
        let schema = schema_for!(SummarizeParams);
        let schema_value =
            serde_json::to_value(schema).expect("Failed to serialize SummarizeParams schema");

        vec![Tool::new(
            "summarize",
            "Load files/directories deterministically and get an LLM summary in a single call. \
             More efficient than subagent when you know what to analyze. \
             Specify paths (files or dirs that will be recursively expanded, respecting top-level .gitignore), \
             a question to focus the summary, and optionally filter by file extensions.",
            schema_value.as_object().unwrap().clone(),
        )]
    }
}

#[async_trait]
impl McpClientTrait for SummarizeClient {
    async fn list_tools(
        &self,
        _session_id: &str,
        _next_cursor: Option<String>,
        _cancellation_token: CancellationToken,
    ) -> Result<ListToolsResult, Error> {
        Ok(ListToolsResult {
            tools: Self::get_tools(),
            next_cursor: None,
            meta: None,
            ..Default::default()
        })
    }

    async fn call_tool(
        &self,
        ctx: &ToolCallContext,
        name: &str,
        arguments: Option<JsonObject>,
        _cancellation_token: CancellationToken,
    ) -> Result<CallToolResult, Error> {
        if name != "summarize" {
            return Ok(CallToolResult::error(vec![ContentBlock::text(format!(
                "Error: Unknown tool: {}",
                name
            ))]));
        }

        let Some(working_dir) = ctx.working_dir_str() else {
            return Ok(CallToolResult::error(vec![ContentBlock::text(
                "Error: working_dir is required for summarize",
            )]));
        };
        let working_dir = PathBuf::from(working_dir);

        let args_value = arguments
            .map(serde_json::Value::Object)
            .unwrap_or(serde_json::Value::Null);

        let params: SummarizeParams = match serde_json::from_value(args_value) {
            Ok(p) => p,
            Err(e) => {
                return Ok(CallToolResult::error(vec![ContentBlock::text(format!(
                    "Error: Invalid parameters: {}",
                    e
                ))]));
            }
        };

        if params.paths.is_empty() {
            return Ok(CallToolResult::error(vec![ContentBlock::text(
                "Error: Must provide at least one path",
            )]));
        }

        let provider = match self.get_provider().await {
            Ok(p) => p,
            Err(e) => {
                return Ok(CallToolResult::error(vec![ContentBlock::text(format!(
                    "Error: {}",
                    e
                ))]));
            }
        };

        let session_id = &ctx.session_id;
        let model_config = match self.context.model_config_for_session(session_id).await {
            Ok(config) => config,
            Err(e) => {
                return Ok(CallToolResult::error(vec![ContentBlock::text(format!(
                    "Error: {}",
                    e
                ))]));
            }
        };
        match execute_summarize(provider, model_config, session_id, params, &working_dir).await {
            Ok(result) => Ok(result),
            Err(msg) => Ok(CallToolResult::error(vec![ContentBlock::text(format!(
                "Error: {}",
                msg
            ))])),
        }
    }

    fn get_info(&self) -> Option<&InitializeResult> {
        Some(&self.info)
    }
}

async fn execute_summarize(
    provider: Arc<dyn Provider>,
    model_config: bcaip_provider_types::model::ModelConfig,
    session_id: &str,
    params: SummarizeParams,
    working_dir: &Path,
) -> Result<CallToolResult, String> {
    let gitignore = build_gitignore(working_dir);
    let files = collect_files(&params.paths, working_dir, &params.extensions, &gitignore)?;

    if files.is_empty() {
        return Err("No files found matching the specified paths and extensions.".to_string());
    }

    let prompt = build_prompt(&files, &params.question, working_dir);
    let total_lines: usize = files.iter().map(|f| f.lines).sum();
    let file_count = files.len();

    let system = "You are an assistant that analyzes content and provides clear, concise summaries \
         focused on answering the user's specific question. \
         Be specific and reference relevant parts of the content when helpful.";

    let user_message = Message::user().with_text(&prompt);

    let (response, _usage) = crate::session_context::with_session_id(
        Some(session_id.to_string()),
        provider.complete(&model_config, system, &[user_message], &[]),
    )
    .await
    .map_err(|e| format!("LLM call failed: {}", e))?;

    let response_text = response
        .content
        .iter()
        .filter_map(|c| {
            if let bcaip_provider_types::conversations::MessageContent::Text(t) = c {
                Some(t.text.clone())
            } else {
                None
            }
        })
        .collect::<Vec<_>>()
        .join("\n");

    let metadata = format!(
        "\n\n---\n*Analyzed {} files ({} lines)*",
        file_count, total_lines
    );

    Ok(CallToolResult::success(vec![ContentBlock::text(format!(
        "{}{}",
        response_text, metadata
    ))]))
}

#[derive(Debug)]
struct FileContent {
    path: PathBuf,
    content: String,
    lines: usize,
}

fn build_gitignore(working_dir: &Path) -> Gitignore {
    let mut builder = GitignoreBuilder::new(working_dir);

    let gitignore_path = working_dir.join(".gitignore");
    if gitignore_path.is_file() {
        let _ = builder.add(&gitignore_path);
    }

    let _ = builder.add_line(None, ".git/");

    builder.build().unwrap_or_else(|_| Gitignore::empty())
}

fn should_include_file(path: &Path, extensions: &Option<Vec<String>>) -> bool {
    match extensions {
        Some(exts) => {
            let ext = match path.extension().and_then(|e| e.to_str()) {
                Some(e) => e.to_lowercase(),
                None => return false,
            };
            exts.iter().any(|e| e.to_lowercase() == ext)
        }
        None => true,
    }
}

fn collect_files(
    paths: &[String],
    working_dir: &Path,
    extensions: &Option<Vec<String>>,
    gitignore: &Gitignore,
) -> Result<Vec<FileContent>, String> {
    let base = working_dir.canonicalize().map_err(|e| {
        format!(
            "Failed to canonicalize working directory {}: {e}",
            working_dir.display()
        )
    })?;
    let mut files = Vec::new();
    let mut total_size: usize = 0;

    for path_str in paths {
        if Path::new(path_str).is_absolute() {
            return Err(format!("Absolute paths are not allowed: {}", path_str));
        }

        let joined_path = base.join(path_str);
        if !joined_path.exists() {
            return Err(format!("Path does not exist: {}", joined_path.display()));
        }

        let canonical_path = joined_path
            .canonicalize()
            .map_err(|e| format!("Failed to canonicalize path {}: {e}", joined_path.display()))?;

        if !canonical_path.starts_with(&base) {
            return Err(format!(
                "Path escapes working directory: {}",
                joined_path.display()
            ));
        }

        if canonical_path.is_dir() {
            collect_from_dir(
                &canonical_path,
                &mut files,
                extensions,
                gitignore,
                &mut total_size,
            )?;
        } else if canonical_path.is_file() && should_include_file(&canonical_path, extensions) {
            collect_file(&canonical_path, &mut files, &mut total_size)?;
        }
    }

    Ok(files)
}

fn collect_from_dir(
    dir: &Path,
    files: &mut Vec<FileContent>,
    extensions: &Option<Vec<String>>,
    gitignore: &Gitignore,
    total_size: &mut usize,
) -> Result<(), String> {
    let entries = std::fs::read_dir(dir).map_err(|e| format!("Failed to read directory: {}", e))?;
    let mut entries: Vec<_> = entries.filter_map(|e| e.ok()).collect();
    entries.sort_by_key(|e| e.file_name());

    for entry in entries {
        let path = entry.path();

        let metadata = match std::fs::symlink_metadata(&path) {
            Ok(m) => m,
            Err(e) => {
                tracing::debug!("Skipping {}: failed to get metadata: {}", path.display(), e);
                continue;
            }
        };

        if metadata.file_type().is_symlink() {
            continue;
        }

        let is_dir = metadata.is_dir();
        let is_file = metadata.is_file();

        if gitignore.matched(&path, is_dir).is_ignore() {
            continue;
        }

        if is_dir {
            collect_from_dir(&path, files, extensions, gitignore, total_size)?;
        } else if is_file && should_include_file(&path, extensions) {
            collect_file(&path, files, total_size)?;
        }
    }

    Ok(())
}

fn collect_file(
    path: &Path,
    files: &mut Vec<FileContent>,
    total_size: &mut usize,
) -> Result<(), String> {
    let metadata = std::fs::metadata(path)
        .map_err(|e| format!("Failed to get metadata for {}: {}", path.display(), e))?;

    if metadata.len() > MAX_FILE_SIZE {
        tracing::debug!(
            "Skipping large file {} ({}KB)",
            path.display(),
            metadata.len() / 1024
        );
        return Ok(());
    }

    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            tracing::debug!("Skipping file {}: {}", path.display(), e);
            return Ok(());
        }
    };

    *total_size += content.len();
    if *total_size > MAX_TOTAL_SIZE {
        tracing::debug!("Total content size limit reached, stopping collection");
        return Err(format!(
            "Total content size limit exceeded: {} bytes over the {} byte limit",
            *total_size - MAX_TOTAL_SIZE,
            MAX_TOTAL_SIZE
        ));
    }

    let lines = content.lines().count();
    files.push(FileContent {
        path: path.to_owned(),
        content,
        lines,
    });

    Ok(())
}

fn build_prompt(files: &[FileContent], question: &str, working_dir: &Path) -> String {
    let total_lines: usize = files.iter().map(|f| f.lines).sum();
    let mut prompt =
        String::with_capacity(files.iter().map(|f| f.content.len()).sum::<usize>() + 1000);

    prompt.push_str(&format!("Answer this question: {}\n\n", question));
    prompt.push_str(&format!(
        "**Files** ({} files, {} total lines):\n\n",
        files.len(),
        total_lines
    ));

    for file in files {
        let display_path = file.path.strip_prefix(working_dir).unwrap_or(&file.path);
        let ext = file.path.extension().and_then(|e| e.to_str()).unwrap_or("");

        prompt.push_str(&format!(
            "### {} ({} lines)\n~~~{}\n{}\n~~~\n\n",
            display_path.display(),
            file.lines,
            ext,
            file.content
        ));
    }

    prompt
}
