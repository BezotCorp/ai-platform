use etcetera::{AppStrategy, choose_app_strategy};
use indoc::formatdoc;
use rmcp::{
    RoleServer, ServerHandler,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{
        CallToolResult, ContentBlock, ErrorCode, ErrorData, Implementation, InitializeResult,
        MetaObject, ServerCapabilities, ServerConfig,
    },
    service::RequestContext,
    tool, tool_handler, tool_router,
};
use std::{
    collections::HashMap,
    fs,
    io::{self, Read, Write},
    path::PathBuf,
};

use crate::memory::{
    RememberMemoryParams, RemoveMemoryCategoryParams, RemoveSpecificMemoryParams,
    RetrieveMemoriesParams,
};

const WORKING_DIR_HEADER: &str = "agent-working-dir";

fn is_reserved_windows_category(category: &str) -> bool {
    let basename = category
        .split('.')
        .next()
        .unwrap_or(category)
        .trim_end_matches([' ', '.']);
    let uppercase = basename.to_ascii_uppercase();

    matches!(uppercase.as_str(), "CON" | "PRN" | "AUX" | "NUL" | "CLOCK$")
        || ["COM", "LPT"].iter().any(|prefix| {
            uppercase.strip_prefix(prefix).is_some_and(|suffix| {
                matches!(
                    suffix,
                    "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
                )
            })
        })
}

fn extract_working_dir_from_meta(meta: &MetaObject) -> Option<PathBuf> {
    meta.0
        .get(WORKING_DIR_HEADER)
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
}

fn memory_error(error: io::Error) -> ErrorData {
    let code = if error.kind() == io::ErrorKind::InvalidInput {
        ErrorCode::INVALID_PARAMS
    } else {
        ErrorCode::INTERNAL_ERROR
    };
    ErrorData::new(code, error.to_string(), None)
}

/// Memory MCP Server using official RMCP SDK
#[derive(Clone)]
pub struct MemoryServer {
    tool_router: ToolRouter<Self>,
    instructions: String,
    global_memory_dir: PathBuf,
}

impl Default for MemoryServer {
    fn default() -> Self {
        Self::new()
    }
}

#[tool_router(router = tool_router)]
impl MemoryServer {
    pub fn new() -> Self {
        let instructions = formatdoc! {r#"
             This extension stores and retrieves categorized information with tagging support.

             Storage:
             - Local: .bcaip/memory/ (project-specific)
             - Global: ~/.config/bcaip/memory/ (user-wide)

             Save proactively when users share preferences, project configurations, workflow patterns,
             or recurring commands. Always confirm with the user before saving. Suggest relevant
             categories and tags, and clarify storage scope (local vs global).

             Use category "*" with retrieve_memories or remove_memory_category to access all entries.
            "#};

        let global_memory_dir = choose_app_strategy(crate::APP_STRATEGY.clone())
            .map(|strategy| strategy.in_config_dir("memory"))
            .unwrap_or_else(|_| PathBuf::from(".config/bcaip/memory"));

        let mut memory_router = Self {
            tool_router: Self::tool_router(),
            instructions: instructions.clone(),
            global_memory_dir,
        };

        let retrieved_global_memories = memory_router.retrieve_all(true, None);

        let mut updated_instructions = instructions;

        let memories_follow_up_instructions = formatdoc! {r#"
            **Here are the user's currently saved memories:**
            Please keep this information in mind when answering future questions.
            Do not bring up memories unless relevant.
            Note: if the user has not saved any memories, this section will be empty.
            Note: if the user removes a memory that was previously loaded into the system, please remove it from the system instructions.
            "#};

        updated_instructions.push_str("\n\n");
        updated_instructions.push_str(&memories_follow_up_instructions);

        if let Ok(global_memories) = retrieved_global_memories
            && !global_memories.is_empty()
        {
            updated_instructions.push_str("\n\nGlobal Memories:\n");
            for (category, memories) in global_memories {
                updated_instructions.push_str(&format!("\nCategory: {}\n", category));
                for memory in memories {
                    updated_instructions.push_str(&format!("- {}\n", memory));
                }
            }
        }

        memory_router.set_instructions(updated_instructions);

        memory_router
    }

    pub fn set_instructions(&mut self, new_instructions: String) {
        self.instructions = new_instructions;
    }

    pub fn get_instructions(&self) -> &str {
        &self.instructions
    }

    fn get_memory_file(
        &self,
        category: &str,
        is_global: bool,
        working_dir: Option<&PathBuf>,
    ) -> io::Result<PathBuf> {
        if category.is_empty()
            || category == "*"
            || category == "."
            || category == ".."
            || category.contains('/')
            || category.contains('\\')
            || category.contains(':')
            || is_reserved_windows_category(category)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "memory category must be a single filename component",
            ));
        }

        let base_dir = if is_global {
            self.global_memory_dir.clone()
        } else {
            let local_base = working_dir
                .cloned()
                .or_else(|| std::env::current_dir().ok())
                .unwrap_or_else(|| PathBuf::from("."));
            local_base.join(".bcaip").join("memory")
        };
        Ok(base_dir.join(format!("{}.txt", category)))
    }

    pub fn retrieve_all(
        &self,
        is_global: bool,
        working_dir: Option<&PathBuf>,
    ) -> io::Result<HashMap<String, Vec<String>>> {
        let base_dir = if is_global {
            self.global_memory_dir.clone()
        } else {
            let local_base = working_dir
                .cloned()
                .or_else(|| std::env::current_dir().ok())
                .unwrap_or_else(|| PathBuf::from("."));
            local_base.join(".bcaip").join("memory")
        };
        let mut memories = HashMap::new();
        if base_dir.exists() {
            for entry in fs::read_dir(&base_dir)? {
                let entry = entry?;
                if entry.file_type()?.is_file() {
                    let file_name = entry.file_name();
                    let Some(category) = file_name
                        .to_str()
                        .and_then(|name| name.strip_suffix(".txt"))
                    else {
                        continue;
                    };
                    if self
                        .get_memory_file(category, is_global, working_dir)
                        .is_err()
                    {
                        continue;
                    }
                    let category_memories = self.retrieve(category, is_global, working_dir)?;
                    memories.insert(
                        category.to_string(),
                        category_memories.into_values().flatten().collect(),
                    );
                }
            }
        }
        Ok(memories)
    }

    pub fn remember(
        &self,
        _context: &str,
        category: &str,
        data: &str,
        tags: &[&str],
        is_global: bool,
        working_dir: Option<&PathBuf>,
    ) -> io::Result<()> {
        let memory_file_path = self.get_memory_file(category, is_global, working_dir)?;

        if let Some(parent) = memory_file_path.parent() {
            fs::create_dir_all(parent)?;
        }

        let mut file = fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(&memory_file_path)?;
        if !tags.is_empty() {
            writeln!(file, "# {}", tags.join(" "))?;
        }
        writeln!(file, "{}\n", data)?;

        Ok(())
    }

    pub fn retrieve(
        &self,
        category: &str,
        is_global: bool,
        working_dir: Option<&PathBuf>,
    ) -> io::Result<HashMap<String, Vec<String>>> {
        let memory_file_path = self.get_memory_file(category, is_global, working_dir)?;
        if !memory_file_path.exists() {
            return Ok(HashMap::new());
        }

        let mut file = fs::File::open(memory_file_path)?;
        let mut content = String::new();
        file.read_to_string(&mut content)?;

        let mut memories = HashMap::new();
        for entry in content.split("\n\n") {
            let mut lines = entry.lines();
            if let Some(first_line) = lines.next() {
                if let Some(stripped) = first_line.strip_prefix('#') {
                    let tags = stripped
                        .split_whitespace()
                        .map(String::from)
                        .collect::<Vec<_>>();
                    memories.insert(tags.join(" "), lines.map(String::from).collect());
                } else {
                    let entry_data: Vec<String> = std::iter::once(first_line.to_string())
                        .chain(lines.map(String::from))
                        .collect();
                    memories
                        .entry("untagged".to_string())
                        .or_insert_with(Vec::new)
                        .extend(entry_data);
                }
            }
        }

        Ok(memories)
    }

    pub fn remove_specific_memory_internal(
        &self,
        category: &str,
        memory_content: &str,
        is_global: bool,
        working_dir: Option<&PathBuf>,
    ) -> io::Result<()> {
        let memory_file_path = self.get_memory_file(category, is_global, working_dir)?;
        if !memory_file_path.exists() {
            return Ok(());
        }

        let mut file = fs::File::open(&memory_file_path)?;
        let mut content = String::new();
        file.read_to_string(&mut content)?;

        let memories: Vec<&str> = content.split("\n\n").collect();
        let new_content: Vec<String> = memories
            .into_iter()
            .filter(|entry| !entry.contains(memory_content))
            .map(|s| s.to_string())
            .collect();

        fs::write(memory_file_path, new_content.join("\n\n"))?;

        Ok(())
    }

    pub fn clear_memory(
        &self,
        category: &str,
        is_global: bool,
        working_dir: Option<&PathBuf>,
    ) -> io::Result<()> {
        let memory_file_path = self.get_memory_file(category, is_global, working_dir)?;
        if memory_file_path.exists() {
            fs::remove_file(memory_file_path)?;
        }

        Ok(())
    }

    pub fn clear_all_global_or_local_memories(
        &self,
        is_global: bool,
        working_dir: Option<&PathBuf>,
    ) -> io::Result<()> {
        let base_dir = if is_global {
            self.global_memory_dir.clone()
        } else {
            let local_base = working_dir
                .cloned()
                .or_else(|| std::env::current_dir().ok())
                .unwrap_or_else(|| PathBuf::from("."));
            local_base.join(".bcaip").join("memory")
        };
        if base_dir.exists() {
            fs::remove_dir_all(&base_dir)?;
        }
        Ok(())
    }

    #[tool(
        name = "remember_memory",
        description = "Stores a memory with optional tags in a specified category"
    )]
    pub async fn remember_memory(
        &self,
        params: Parameters<RememberMemoryParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let params = params.0;
        let working_dir = extract_working_dir_from_meta(&context.meta);

        if params.data.is_empty() {
            return Err(ErrorData::new(
                ErrorCode::INVALID_PARAMS,
                "Data must not be empty when remembering a memory".to_string(),
                None,
            ));
        }

        let tags: Vec<&str> = params.tags.iter().map(|s| s.as_str()).collect();
        self.remember(
            "context",
            &params.category,
            &params.data,
            &tags,
            params.is_global,
            working_dir.as_ref(),
        )
        .map_err(memory_error)?;

        Ok(CallToolResult::success(vec![ContentBlock::text(format!(
            "Stored memory in category: {}",
            params.category
        ))]))
    }

    #[tool(
        name = "retrieve_memories",
        description = "Retrieves all memories from a specified category"
    )]
    pub async fn retrieve_memories(
        &self,
        params: Parameters<RetrieveMemoriesParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let params = params.0;
        let working_dir = extract_working_dir_from_meta(&context.meta);

        let memories = if params.category == "*" {
            self.retrieve_all(params.is_global, working_dir.as_ref())
        } else {
            self.retrieve(&params.category, params.is_global, working_dir.as_ref())
        }
        .map_err(memory_error)?;

        Ok(CallToolResult::success(vec![ContentBlock::text(format!(
            "Retrieved memories: {:?}",
            memories
        ))]))
    }

    #[tool(
        name = "remove_memory_category",
        description = "Removes all memories within a specified category"
    )]
    pub async fn remove_memory_category(
        &self,
        params: Parameters<RemoveMemoryCategoryParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let params = params.0;
        let working_dir = extract_working_dir_from_meta(&context.meta);

        let message = if params.category == "*" {
            self.clear_all_global_or_local_memories(params.is_global, working_dir.as_ref())
                .map_err(memory_error)?;
            format!(
                "Cleared all memory {} categories",
                if params.is_global { "global" } else { "local" }
            )
        } else {
            self.clear_memory(&params.category, params.is_global, working_dir.as_ref())
                .map_err(memory_error)?;
            format!("Cleared memories in category: {}", params.category)
        };

        Ok(CallToolResult::success(vec![ContentBlock::text(message)]))
    }

    #[tool(
        name = "remove_specific_memory",
        description = "Removes a specific memory within a specified category"
    )]
    pub async fn remove_specific_memory(
        &self,
        params: Parameters<RemoveSpecificMemoryParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let params = params.0;
        let working_dir = extract_working_dir_from_meta(&context.meta);

        self.remove_specific_memory_internal(
            &params.category,
            &params.memory_content,
            params.is_global,
            working_dir.as_ref(),
        )
        .map_err(memory_error)?;

        Ok(CallToolResult::success(vec![ContentBlock::text(format!(
            "Removed specific memory from category: {}",
            params.category
        ))]))
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for MemoryServer {
    fn get_info(&self) -> ServerConfig {
        InitializeResult::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new(
                "bcaip-memory",
                env!("CARGO_PKG_VERSION"),
            ))
            .with_instructions(self.instructions.clone())
    }
}
