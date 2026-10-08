use crate::agents::mcp_client::{Error, McpClientTrait};
use crate::agents::{ToolCallContext, extension::PlatformExtensionContext};
use crate::config::Config;
use crate::skills::{discover_skills_with_config, loaded_skill_context_with_args};
use async_trait::async_trait;
use bcaip_sdk_types::custom_requests::{SourceEntry, SourceType};
use rmcp::model::{
    CallToolResult, ContentBlock, Implementation, InitializeResult, JsonObject, ListToolsResult,
    ServerCapabilities, ServerNotification, Tool,
};
use std::path::{Path, PathBuf};
use std::sync::RwLock;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
pub static EXTENSION_NAME: &str = "skills";

pub struct SkillsClient {
    info: InitializeResult,
    working_dir: RwLock<PathBuf>,
    exclude_builtin_skills: bool,
    config: &'static Config,
}

impl SkillsClient {
    pub fn new(context: PlatformExtensionContext) -> anyhow::Result<Self> {
        let working_dir = context
            .session
            .as_ref()
            .map(|s| s.working_dir.clone())
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());

        let info = InitializeResult::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new(EXTENSION_NAME, "1.0.0").with_title("Skills"));

        Ok(Self {
            info,
            working_dir: RwLock::new(working_dir),
            exclude_builtin_skills: false,
            config: Config::global(),
        })
    }

    /// Controls whether BCAIP's bundled skills are exposed by this client.
    /// Bundled skills are enabled by default.
    pub fn with_builtin_skills(mut self, enabled: bool) -> Self {
        self.exclude_builtin_skills = !enabled;
        self
    }

    fn discover_skills(&self) -> Vec<SourceEntry> {
        let working_dir = self.working_dir.read().unwrap().clone();
        discover_skills_with_config(Some(&working_dir), self.config)
            .into_iter()
            .filter(|skill| {
                !self.exclude_builtin_skills || skill.source_type != SourceType::BuiltinSkill
            })
            .collect()
    }
}

#[async_trait]
impl McpClientTrait for SkillsClient {
    async fn list_tools(
        &self,
        _session_id: &str,
        _next_cursor: Option<String>,
        _cancellation_token: CancellationToken,
    ) -> Result<ListToolsResult, Error> {
        let schema = serde_json::json!({
            "type": "object",
            "required": ["name"],
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Name of the skill to load. Use \"skill-name/path\" to load a supporting file."
                },
                "args": {
                    "type": "string",
                    "description": "Optional arguments to provide when loading the skill."
                }
            }
        });

        let tool = Tool::new(
            "load_skill",
            "Load a skill's full content into your context so you can follow its instructions.\n\n\
             Skills are listed in your system instructions. When you need to use one, \
             load it first to get the detailed instructions.\n\n\
             Examples:\n\
             - load_skill(name: \"gdrive\") → Loads the gdrive skill instructions\n\
             - load_skill(name: \"my-skill\", args: \"the arguments for the skill\") → Loads a skill with arguments\n\
             - load_skill(name: \"my-skill/template.md\") → Loads a supporting file"
                .to_string(),
            schema.as_object().unwrap().clone(),
        );

        Ok(ListToolsResult {
            tools: vec![tool],
            next_cursor: None,
            meta: None,
            ..Default::default()
        })
    }

    async fn call_tool(
        &self,
        _ctx: &ToolCallContext,
        name: &str,
        arguments: Option<JsonObject>,
        _cancellation_token: CancellationToken,
    ) -> Result<CallToolResult, Error> {
        if name != "load_skill" {
            return Ok(CallToolResult::error(vec![ContentBlock::text(format!(
                "Unknown tool: {}",
                name
            ))]));
        }

        let skill_name = arguments
            .as_ref()
            .and_then(|args| args.get("name"))
            .and_then(|v| v.as_str())
            .unwrap_or("");

        if skill_name.is_empty() {
            return Ok(CallToolResult::error(vec![ContentBlock::text(
                "Missing required parameter: name",
            )]));
        }
        let args = arguments
            .as_ref()
            .and_then(|args| args.get("args"))
            .and_then(|v| v.as_str());

        let skills = self.discover_skills();

        if let Some(skill) = skills.iter().find(|s| s.name == skill_name) {
            return match loaded_skill_context_with_args(skill, args) {
                Ok(rendered) => Ok(CallToolResult::success(vec![ContentBlock::text(rendered)])),
                Err(e) => Ok(CallToolResult::error(vec![ContentBlock::text(format!(
                    "Failed to parse skill arguments: {}",
                    e
                ))])),
            };
        }

        if let Some((parent_skill_name, raw_relative_path)) = skill_name.split_once('/') {
            let relative_path = raw_relative_path.replace('\\', "/");
            if let Some(skill) = skills.iter().find(|s| {
                s.name == parent_skill_name
                    && matches!(s.source_type, SourceType::Skill | SourceType::BuiltinSkill)
            }) {
                let listed_skill_dir = PathBuf::from(&skill.path);
                let load_skill_dir = match listed_skill_dir.canonicalize() {
                    Ok(path) => path,
                    Err(e) => {
                        return Ok(CallToolResult::error(vec![ContentBlock::text(format!(
                            "Failed to resolve '{}': {}",
                            parent_skill_name, e
                        ))]));
                    }
                };

                for file_path in &skill.supporting_files {
                    let file_path_buf = Path::new(file_path);
                    let Ok(rel) = file_path_buf.strip_prefix(&listed_skill_dir) else {
                        continue;
                    };
                    if rel.to_string_lossy().replace('\\', "/") != relative_path {
                        continue;
                    }

                    let result = match super::load_supporting_file(&load_skill_dir, rel, skill_name)
                    {
                        Ok(content) => CallToolResult::success(vec![ContentBlock::text(content)]),
                        Err(e) => CallToolResult::error(vec![ContentBlock::text(format!(
                            "Failed to read '{}': {}",
                            skill_name, e
                        ))]),
                    };
                    return Ok(result);
                }

                let available: Vec<String> = skill
                    .supporting_files
                    .iter()
                    .filter_map(|f| {
                        Path::new(f)
                            .strip_prefix(&listed_skill_dir)
                            .ok()
                            .map(|r| r.to_string_lossy().replace('\\', "/"))
                    })
                    .take(10)
                    .collect();

                return Ok(if available.is_empty() {
                    CallToolResult::error(vec![ContentBlock::text(format!(
                        "Skill '{}' has no supporting files.",
                        skill.name
                    ))])
                } else {
                    CallToolResult::error(vec![ContentBlock::text(format!(
                        "File '{}' not found. Available: {}",
                        skill_name,
                        available.join(", ")
                    ))])
                });
            }
        }

        let suggestions: Vec<&str> = skills
            .iter()
            .filter(|s| {
                s.name.to_lowercase().contains(&skill_name.to_lowercase())
                    || skill_name.to_lowercase().contains(&s.name.to_lowercase())
            })
            .take(3)
            .map(|s| s.name.as_str())
            .collect();

        Ok(if suggestions.is_empty() {
            CallToolResult::error(vec![ContentBlock::text(format!(
                "Skill '{}' not found.",
                skill_name
            ))])
        } else {
            CallToolResult::error(vec![ContentBlock::text(format!(
                "Skill '{}' not found. Did you mean: {}?",
                skill_name,
                suggestions.join(", ")
            ))])
        })
    }

    fn get_info(&self) -> Option<&InitializeResult> {
        Some(&self.info)
    }

    fn get_instructions(&self) -> Option<String> {
        let sources = self.discover_skills();
        let mut skills: Vec<&SourceEntry> = sources
            .iter()
            .filter(|s| {
                s.source_type == SourceType::Skill || s.source_type == SourceType::BuiltinSkill
            })
            .collect();
        skills.sort_by(|a, b| (&a.name, &a.path).cmp(&(&b.name, &b.path)));

        if skills.is_empty() {
            return None;
        }

        let mut instructions = String::from(
            "\n\nYou have these skills at your disposal, when it is clear they can help you solve a problem or you are asked to use them:",
        );
        for skill in &skills {
            instructions.push_str(&format!("\n• {} - {}", skill.name, skill.description));
        }
        Some(instructions)
    }

    async fn update_working_dir(&self, new_dir: PathBuf) -> Result<(), Error> {
        *self.working_dir.write().unwrap() = new_dir;
        Ok(())
    }

    async fn subscribe(&self) -> mpsc::Receiver<ServerNotification> {
        let (_tx, rx) = mpsc::channel(1);
        rx
    }
}
