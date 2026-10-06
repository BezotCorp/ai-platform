use crate::agents::{extension::ExtensionInfo, moim};
use crate::config::Config;
use crate::hints::load_hints::build_gitignore;
use crate::hints::{SubdirectoryHintTracker, get_context_filenames, load_hint_files};
use crate::prompt_template;
use crate::utils::sanitize_unicode_tags;
use chrono::Utc;
use bcaip_provider_types::goose_mode::GooseMode;
use indexmap::IndexMap;
use serde::Serialize;
use std::path::Path;
pub struct PromptManager {
    system_prompt_override: Option<String>,
    system_prompt_extras: IndexMap<String, String>,
    current_date_timestamp: String,
    subdirectory_hint_tracker: SubdirectoryHintTracker,
}

impl Default for PromptManager {
    fn default() -> Self {
        PromptManager::new()
    }
}

#[derive(Serialize)]
struct SystemPromptContext {
    extensions: Vec<ExtensionInfo>,
    current_date_time: String,
    goose_mode: GooseMode,
    is_autonomous: bool,
    enable_subagents: bool,
    code_execution_mode: bool,
    include_extensions: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    moim_system_prompt_block: Option<String>,
}

pub struct SystemPromptBuilder<'a, M> {
    manager: &'a M,

    extensions_info: Vec<ExtensionInfo>,
    prompt_extras: IndexMap<String, String>,
    subagents_enabled: bool,
    hints: Option<String>,
    code_execution_mode: bool,
    include_extensions: bool,
    goose_mode: Option<GooseMode>,
}

impl<'a> SystemPromptBuilder<'a, PromptManager> {
    pub fn with_extension(mut self, extension: ExtensionInfo) -> Self {
        self.extensions_info.push(extension);
        self
    }

    pub fn with_extensions(mut self, extensions: impl Iterator<Item = ExtensionInfo>) -> Self {
        for extension in extensions {
            self.extensions_info.push(extension);
        }
        self
    }

    pub fn with_prompt_extras(
        mut self,
        extras: impl IntoIterator<Item = (String, String)>,
    ) -> Self {
        self.prompt_extras.extend(extras);
        self
    }

    pub fn with_code_execution_mode(mut self, enabled: bool) -> Self {
        self.code_execution_mode = enabled;
        self
    }

    pub fn without_extensions(mut self) -> Self {
        self.include_extensions = false;
        self
    }

    pub fn with_hints(mut self, working_dir: &Path) -> Self {
        let hints_filenames = get_context_filenames();
        let ignore_patterns = build_gitignore(working_dir);

        let hints = load_hint_files(working_dir, &hints_filenames, &ignore_patterns);

        if !hints.is_empty() {
            self.hints = Some(hints);
        }
        self
    }

    pub fn with_enable_subagents(mut self, subagents_enabled: bool) -> Self {
        self.subagents_enabled = subagents_enabled;
        self
    }

    pub fn with_goose_mode(mut self, mode: GooseMode) -> Self {
        self.goose_mode = Some(mode);
        self
    }

    pub fn build(self) -> String {
        let mut extensions_info = self.extensions_info;

        // Stable tool ordering is important for multi session prompt caching.
        extensions_info.sort_by(|a, b| a.name.cmp(&b.name));

        let sanitized_extensions_info: Vec<ExtensionInfo> = extensions_info
            .into_iter()
            .map(|mut ext_info| {
                ext_info.instructions = sanitize_unicode_tags(&ext_info.instructions);
                ext_info
            })
            .collect();

        let goose_mode = self
            .goose_mode
            .unwrap_or_else(|| Config::global().get_goose_mode().unwrap_or_default());

        let context = SystemPromptContext {
            extensions: sanitized_extensions_info,
            current_date_time: self.manager.current_date_timestamp.clone(),
            goose_mode,
            is_autonomous: goose_mode == GooseMode::Auto,
            enable_subagents: self.subagents_enabled,
            code_execution_mode: self.code_execution_mode,
            include_extensions: self.include_extensions,
            moim_system_prompt_block: moim::system_prompt_block(),
        };

        let base_prompt = if let Some(override_prompt) = &self.manager.system_prompt_override {
            let sanitized_override_prompt = sanitize_unicode_tags(override_prompt);
            prompt_template::render_string(&sanitized_override_prompt, &context)
        } else {
            prompt_template::render_template("system.md", &context)
        }
        .unwrap_or_else(|_| {
            "You are a general-purpose AI agent called goose, created by Block".to_string()
        });

        let mut system_prompt_extras = self.manager.system_prompt_extras.clone();
        system_prompt_extras.extend(self.prompt_extras);

        // Add hints if provided
        if let Some(hints) = self.hints {
            system_prompt_extras.insert("hints".to_string(), hints);
        }

        if goose_mode == GooseMode::Chat {
            system_prompt_extras.insert(
                "chat_mode".to_string(),
                "Right now you are in the chat only mode, no access to any tool use and system."
                    .to_string(),
            );
        }

        if system_prompt_extras.is_empty() {
            base_prompt
        } else {
            let sanitized_system_prompt_extras: Vec<String> = system_prompt_extras
                .into_values()
                .map(|extra| sanitize_unicode_tags(&extra))
                .collect();

            format!(
                "{}\n\n# Additional Instructions:\n\n{}",
                base_prompt,
                sanitized_system_prompt_extras.join("\n\n")
            )
        }
    }
}

impl PromptManager {
    pub fn new() -> Self {
        PromptManager {
            system_prompt_override: None,
            system_prompt_extras: IndexMap::new(),
            // Use the fixed current date time so that prompt cache can be used.
            // Filtering to an hour to balance user time accuracy and multi session prompt cache hits.
            current_date_timestamp: Utc::now().format("%Y-%m-%d %H:00 %:z").to_string(),
            subdirectory_hint_tracker: SubdirectoryHintTracker::new(),
        }
    }

    /// Add an additional instruction to the system prompt with a key
    /// Using the same key will replace the previous instruction
    pub fn add_system_prompt_extra(&mut self, key: String, instruction: String) {
        self.system_prompt_extras.insert(key, instruction);
    }

    pub fn remove_system_prompt_extra(&mut self, key: &str) {
        self.system_prompt_extras.shift_remove(key);
    }

    pub fn record_tool_arguments(
        &mut self,
        arguments: &Option<serde_json::Map<String, serde_json::Value>>,
        working_dir: &Path,
    ) {
        self.subdirectory_hint_tracker
            .record_tool_arguments(arguments, working_dir);
    }

    pub fn load_subdirectory_hints(&mut self, working_dir: &Path) -> bool {
        let new_hints = self.subdirectory_hint_tracker.load_new_hints(working_dir);
        let has_new = !new_hints.is_empty();
        for (key, content) in new_hints {
            self.system_prompt_extras.insert(key, content);
        }
        has_new
    }

    pub fn build_system_prompt(
        &mut self,
        working_dir: &Path,
        prompt_parts: Vec<(String, String)>,
        goose_mode: GooseMode,
    ) -> String {
        self.load_subdirectory_hints(working_dir);
        self.builder()
            .with_prompt_extras(prompt_parts)
            .with_hints(working_dir)
            .with_goose_mode(goose_mode)
            .without_extensions()
            .build()
    }

    /// Override the system prompt with custom text
    pub fn set_system_prompt_override(&mut self, template: String) {
        self.system_prompt_override = Some(template);
    }

    pub fn clear_system_prompt_override(&mut self) {
        self.system_prompt_override = None;
    }

    pub fn builder<'a>(&'a self) -> SystemPromptBuilder<'a, Self> {
        SystemPromptBuilder {
            manager: self,

            extensions_info: vec![],
            prompt_extras: IndexMap::new(),
            subagents_enabled: false,
            hints: None,
            code_execution_mode: false,
            include_extensions: true,
            goose_mode: None,
        }
    }
}
