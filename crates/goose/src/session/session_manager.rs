use crate::config::paths::Paths;
use crate::recipe::Recipe;
use crate::recipe::validate_recipe::strip_unreferenced_parameters;
use crate::session::export_markdown::export_session_to_markdown;
use crate::session::extension_data::ExtensionData;
use crate::session::session_naming::{
    MSG_COUNT_FOR_SESSION_NAME_GENERATION, generate_session_name,
};
use anyhow::Result;
use bcaip_provider_types::base::Provider;
use bcaip_provider_types::conversations::CostSource;
use bcaip_provider_types::conversations::{
    Conversation, Message, MessageContent, MessageMetadata, MessageUsage, TokenState, Usage,
};
use bcaip_provider_types::goose_mode::GooseMode;
use bcaip_provider_types::model::ModelConfig;
use chrono::{DateTime, TimeZone, Utc};
use rmcp::model::Role;
use serde::{Deserialize, Serialize};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{AssertSqlSafe, Pool, Sqlite};
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock};
use std::{collections::HashMap, fs};
use tracing::{info, warn};

pub const CURRENT_SCHEMA_VERSION: i32 = 16;
pub const SESSIONS_FOLDER: &str = "sessions";
pub const DB_NAME: &str = "sessions.db";
const MILLISECOND_TIMESTAMP_THRESHOLD: i64 = 10_000_000_000;
const SESSION_COUNT_BATCH_SIZE: usize = 900;

#[derive(
    Debug,
    Clone,
    Copy,
    Serialize,
    Deserialize,
    PartialEq,
    Eq,
    Default,
    strum::Display,
    strum::EnumString,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum SessionType {
    #[default]
    User,
    Scheduled,
    SubAgent,
    Hidden,
    Terminal,
    Gateway,
    Acp,
}

static SESSION_STORAGE: LazyLock<Arc<SessionStorage>> =
    LazyLock::new(|| Arc::new(SessionStorage::new(Paths::data_dir())));

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub working_dir: PathBuf,
    #[serde(alias = "description")]
    pub name: String,
    #[serde(default)]
    pub user_set_name: bool,
    #[serde(default)]
    pub session_type: SessionType,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub extension_data: ExtensionData,
    #[serde(default)]
    pub usage: Usage,
    #[serde(default)]
    pub accumulated_usage: Usage,
    pub accumulated_cost: Option<f64>,
    pub schedule_id: Option<String>,
    pub recipe: Option<Recipe>,
    pub user_recipe_values: Option<HashMap<String, String>>,
    pub conversation: Option<Conversation>,
    pub message_count: usize,
    #[serde(default)]
    pub last_message_at: Option<DateTime<Utc>>,
    pub provider_name: Option<String>,
    pub model_config: Option<ModelConfig>,
    #[serde(default)]
    pub goose_mode: GooseMode,
    #[serde(default)]
    pub archived_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub parent_session_id: Option<String>,
    #[serde(default)]
    pub last_message_snippet: Option<String>,
}

impl From<&Session> for TokenState {
    fn from(session: &Session) -> Self {
        Self {
            input_tokens: session.usage.input_tokens.unwrap_or(0),
            output_tokens: session.usage.output_tokens.unwrap_or(0),
            total_tokens: session.usage.total_tokens.unwrap_or(0),
            cache_read_tokens: session.usage.cache_read_input_tokens.unwrap_or(0),
            cache_write_tokens: session.usage.cache_write_input_tokens.unwrap_or(0),
            accumulated_input_tokens: session.accumulated_usage.input_tokens.unwrap_or(0),
            accumulated_output_tokens: session.accumulated_usage.output_tokens.unwrap_or(0),
            accumulated_total_tokens: session.accumulated_usage.total_tokens.unwrap_or(0),
            accumulated_cache_read_tokens: session
                .accumulated_usage
                .cache_read_input_tokens
                .unwrap_or(0),
            accumulated_cache_write_tokens: session
                .accumulated_usage
                .cache_write_input_tokens
                .unwrap_or(0),
            accumulated_cost: session.accumulated_cost,
        }
    }
}

pub fn token_state_from_session_and_totals(
    session: &Session,
    totals: &SessionUsageTotals,
) -> TokenState {
    TokenState {
        input_tokens: session.usage.input_tokens.unwrap_or(0),
        output_tokens: session.usage.output_tokens.unwrap_or(0),
        total_tokens: session.usage.total_tokens.unwrap_or(0),
        cache_read_tokens: session.usage.cache_read_input_tokens.unwrap_or(0),
        cache_write_tokens: session.usage.cache_write_input_tokens.unwrap_or(0),
        accumulated_input_tokens: totals.accumulated_usage.input_tokens.unwrap_or(0),
        accumulated_output_tokens: totals.accumulated_usage.output_tokens.unwrap_or(0),
        accumulated_total_tokens: totals.accumulated_usage.total_tokens.unwrap_or(0),
        accumulated_cache_read_tokens: totals
            .accumulated_usage
            .cache_read_input_tokens
            .unwrap_or(0),
        accumulated_cache_write_tokens: totals
            .accumulated_usage
            .cache_write_input_tokens
            .unwrap_or(0),
        accumulated_cost: totals.accumulated_cost,
    }
}

pub struct SessionUpdateBuilder<'a> {
    session_manager: &'a SessionManager,
    session_id: String,
    name: Option<String>,
    user_set_name: Option<bool>,
    session_type: Option<SessionType>,
    working_dir: Option<PathBuf>,
    extension_data: Option<ExtensionData>,
    usage: Option<Usage>,
    accumulated_usage: Option<Usage>,
    accumulated_cost: Option<Option<f64>>,
    schedule_id: Option<Option<String>>,
    recipe: Option<Option<Recipe>>,
    user_recipe_values: Option<Option<HashMap<String, String>>>,
    provider_name: Option<Option<String>>,
    model_config: Option<Option<ModelConfig>>,
    goose_mode: Option<GooseMode>,
    archived_at: Option<Option<DateTime<Utc>>>,

    project_id: Option<Option<String>>,
    parent_session_id: Option<Option<String>>,
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SessionInsights {
    pub total_sessions: usize,
    pub total_tokens: i64,
}

#[derive(Debug, Clone, Default)]
pub struct SessionUsageTotals {
    pub accumulated_usage: Usage,
    pub accumulated_cost: Option<f64>,
}

impl<'a> SessionUpdateBuilder<'a> {
    fn new(session_manager: &'a SessionManager, session_id: String) -> Self {
        Self {
            session_manager,
            session_id,
            name: None,
            user_set_name: None,
            session_type: None,
            working_dir: None,
            extension_data: None,
            usage: None,
            accumulated_usage: None,
            accumulated_cost: None,
            schedule_id: None,
            recipe: None,
            user_recipe_values: None,
            provider_name: None,
            model_config: None,
            goose_mode: None,
            archived_at: None,
            project_id: None,
            parent_session_id: None,
        }
    }

    pub async fn apply(self) -> Result<()> {
        self.session_manager.apply_update_inner(self).await
    }

    pub fn user_provided_name(mut self, name: impl Into<String>) -> Self {
        let name = name.into().trim().to_string();
        if !name.is_empty() {
            self.name = Some(name);
            self.user_set_name = Some(true);
        }
        self
    }

    pub fn system_generated_name(mut self, name: impl Into<String>) -> Self {
        let name = name.into().trim().to_string();
        if !name.is_empty() {
            self.name = Some(name);
            self.user_set_name = Some(false);
        }
        self
    }

    pub fn session_type(mut self, session_type: SessionType) -> Self {
        self.session_type = Some(session_type);
        self
    }

    pub fn working_dir(mut self, working_dir: PathBuf) -> Self {
        self.working_dir = Some(working_dir);
        self
    }

    pub fn extension_data(mut self, data: ExtensionData) -> Self {
        self.extension_data = Some(data);
        self
    }

    pub fn usage(mut self, usage: Usage) -> Self {
        self.usage = Some(usage);
        self
    }

    pub fn accumulated_usage(mut self, usage: Usage) -> Self {
        self.accumulated_usage = Some(usage);
        self
    }

    pub fn accumulated_cost(mut self, cost: Option<f64>) -> Self {
        self.accumulated_cost = Some(cost);
        self
    }

    pub fn schedule_id(mut self, schedule_id: Option<String>) -> Self {
        self.schedule_id = Some(schedule_id);
        self
    }

    pub fn recipe(mut self, recipe: Option<Recipe>) -> Self {
        self.recipe = Some(recipe.map(strip_unreferenced_parameters));
        self
    }

    pub fn user_recipe_values(
        mut self,
        user_recipe_values: Option<HashMap<String, String>>,
    ) -> Self {
        self.user_recipe_values = Some(user_recipe_values);
        self
    }

    pub fn provider_name(mut self, provider_name: impl Into<String>) -> Self {
        self.provider_name = Some(Some(provider_name.into()));
        self
    }

    pub fn model_config(mut self, model_config: ModelConfig) -> Self {
        self.model_config = Some(Some(model_config));
        self
    }

    pub fn clear_model_config(mut self) -> Self {
        self.model_config = Some(None);
        self
    }

    pub fn goose_mode(mut self, mode: GooseMode) -> Self {
        self.goose_mode = Some(mode);
        self
    }

    pub fn archived_at(mut self, archived_at: Option<DateTime<Utc>>) -> Self {
        self.archived_at = Some(archived_at);
        self
    }

    pub fn project_id(mut self, project_id: Option<String>) -> Self {
        self.project_id = Some(project_id);
        self
    }

    pub fn parent_session_id(mut self, parent_session_id: Option<String>) -> Self {
        self.parent_session_id = Some(parent_session_id);
        self
    }
}

pub struct SessionManager {
    storage: Arc<SessionStorage>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SessionListCursor {
    pub(crate) sort_at: DateTime<Utc>,
    pub(crate) session_id: String,
}

#[derive(Debug, Clone)]
pub(crate) struct SessionListPage {
    pub(crate) sessions: Vec<Session>,
    pub(crate) next_cursor: Option<SessionListCursor>,
}

#[derive(Debug, Default, Clone)]
pub(crate) struct SessionListFilters<'a> {
    pub(crate) types: Option<&'a [SessionType]>,
    pub(crate) working_dir: Option<&'a Path>,
    pub(crate) keyword: Option<&'a str>,
    pub(crate) only_sessions_with_messages: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct SessionListPageQuery<'a> {
    pub(crate) filters: SessionListFilters<'a>,
    pub(crate) cursor: Option<&'a SessionListCursor>,
    pub(crate) page_size: usize,
    pub(crate) include_last_message_snippet: bool,
}

#[derive(Debug, Default)]
struct SessionListQuery<'a> {
    filters: SessionListFilters<'a>,
    cursor: Option<&'a SessionListCursor>,
    limit: Option<usize>,
}

fn keyword_terms(query: Option<&str>) -> Vec<String> {
    query
        .unwrap_or_default()
        .split_whitespace()
        .map(|word| word.to_lowercase())
        .collect()
}

fn message_keyword_clause(keyword_count: usize) -> String {
    let keyword_clauses = (0..keyword_count)
        .map(|_| "instr(LOWER(json_extract(value, '$.text')), ?) > 0")
        .collect::<Vec<_>>()
        .join(" OR ");

    let visible = user_visible_message_sql("mq.metadata_json");
    format!(
        r#"
        EXISTS (
            SELECT 1
            FROM messages mq
            WHERE mq.session_id = s.id
              AND {visible}
              AND EXISTS (
                  SELECT 1
                  FROM json_each(mq.content_json)
                  WHERE json_extract(value, '$.type') = 'text'
                    AND ({keyword_clauses})
              )
        )
        "#
    )
}

#[derive(Debug, Clone)]
pub struct SessionNameUpdate {
    pub session_id: String,
    pub name: String,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub message_count: usize,
    pub user_set_name: bool,
}

impl SessionManager {
    pub fn new(data_dir: PathBuf) -> Self {
        Self {
            storage: Arc::new(SessionStorage::new(data_dir)),
        }
    }

    pub fn instance() -> Self {
        Self {
            storage: Arc::clone(&SESSION_STORAGE),
        }
    }

    pub fn storage(&self) -> &Arc<SessionStorage> {
        &self.storage
    }

    pub(crate) fn action_required(
        &self,
    ) -> Arc<crate::action_required_manager::ActionRequiredManager> {
        self.storage.action_required.clone()
    }

    pub async fn create_session(
        &self,
        working_dir: PathBuf,
        name: String,
        session_type: SessionType,
        goose_mode: GooseMode,
    ) -> Result<Session> {
        self.storage
            .create_session(working_dir, name, session_type, goose_mode)
            .await
    }

    pub async fn get_session(&self, id: &str, include_messages: bool) -> Result<Session> {
        self.storage.get_session(id, include_messages).await
    }

    pub fn update(&self, id: &str) -> SessionUpdateBuilder<'_> {
        SessionUpdateBuilder::new(self, id.to_string())
    }

    pub(crate) async fn update_project_for_session_types(
        &self,
        id: &str,
        project_id: Option<String>,
        session_types: &[SessionType],
    ) -> Result<bool> {
        self.storage
            .update_project_for_session_types(id, project_id, session_types)
            .await
    }

    async fn apply_update_inner(&self, builder: SessionUpdateBuilder<'_>) -> Result<()> {
        self.storage.apply_update(builder).await
    }

    pub async fn add_message(&self, id: &str, message: &Message) -> Result<()> {
        self.storage.add_message(id, message).await
    }

    pub async fn replace_conversation(&self, id: &str, conversation: &Conversation) -> Result<()> {
        self.storage.replace_conversation(id, conversation).await
    }

    pub(crate) async fn save_compacted_conversation(
        &self,
        id: &str,
        conversation: &Conversation,
    ) -> Result<()> {
        self.storage
            .save_compacted_conversation(id, conversation)
            .await
    }

    pub async fn list_sessions(&self) -> Result<Vec<Session>> {
        self.storage.list_sessions().await
    }

    pub async fn list_sessions_with_limit(&self, limit: usize) -> Result<Vec<Session>> {
        self.storage
            .list_sessions_matching(SessionListQuery {
                filters: SessionListFilters {
                    types: Some(&[SessionType::User, SessionType::Scheduled]),
                    ..Default::default()
                },
                limit: Some(limit),
                ..Default::default()
            })
            .await
    }

    pub async fn list_sessions_by_types(&self, types: &[SessionType]) -> Result<Vec<Session>> {
        self.storage.list_sessions_by_types(Some(types)).await
    }

    pub(crate) async fn list_sessions_paged(
        &self,
        query: SessionListPageQuery<'_>,
    ) -> Result<SessionListPage> {
        self.storage.list_sessions_paged(query).await
    }

    pub async fn list_all_sessions(&self) -> Result<Vec<Session>> {
        self.storage.list_sessions_by_types(None).await
    }

    pub async fn delete_session(&self, id: &str) -> Result<()> {
        self.storage.delete_session(id).await
    }

    pub async fn get_insights(&self) -> Result<SessionInsights> {
        self.storage
            .get_insights(&[SessionType::User, SessionType::Scheduled])
            .await
    }

    pub async fn get_session_usage_totals(&self, id: &str) -> Result<SessionUsageTotals> {
        self.storage.get_session_usage_totals(id).await
    }

    pub async fn record_usage_metrics(
        &self,
        session_id: &str,
        schedule_id: Option<String>,
        current_usage: Usage,
        model: &str,
        ledger: &MessageUsage,
    ) -> Result<()> {
        self.storage
            .record_usage_metrics(session_id, schedule_id, current_usage, model, ledger)
            .await
    }

    pub async fn export_session(&self, id: &str) -> Result<String> {
        self.storage.export_session(id).await
    }

    pub async fn export_session_markdown(&self, id: &str) -> Result<String> {
        let session = self.get_session(id, true).await?;
        let messages = session
            .conversation
            .map(|conversation| conversation.user_visible_messages())
            .unwrap_or_default();
        Ok(export_session_to_markdown(messages, &session.name))
    }

    pub async fn import_session(
        &self,
        json: &str,
        session_type_override: Option<SessionType>,
    ) -> Result<Session> {
        self.storage
            .import_session(self, json, session_type_override)
            .await
    }

    pub async fn copy_session(&self, session_id: &str, new_name: String) -> Result<Session> {
        self.storage.copy_session(self, session_id, new_name).await
    }

    pub async fn truncate_conversation(&self, session_id: &str, timestamp: i64) -> Result<()> {
        self.storage
            .truncate_conversation(session_id, timestamp)
            .await
    }

    pub async fn truncate_conversation_from_message(
        &self,
        session_id: &str,
        message_id: &str,
    ) -> Result<()> {
        self.storage
            .truncate_conversation_from_message(session_id, message_id)
            .await
    }

    async fn system_generated_name_update(
        &self,
        id: &str,
        name: String,
    ) -> Result<SessionNameUpdate> {
        self.update(id)
            .system_generated_name(name.clone())
            .apply()
            .await?;

        let session = self.get_session(id, false).await?;
        Ok(SessionNameUpdate {
            session_id: id.to_string(),
            name,
            updated_at: session.updated_at,
            message_count: session.message_count,
            user_set_name: session.user_set_name,
        })
    }

    pub async fn maybe_update_name(
        &self,
        id: &str,
        provider: Arc<dyn Provider>,
    ) -> Result<Option<SessionNameUpdate>> {
        let session = self.get_session(id, true).await?;

        if session.user_set_name {
            return Ok(None);
        }

        if session.session_type == SessionType::Scheduled {
            return Ok(None);
        }

        if let Some(recipe) = &session.recipe {
            let name = recipe.title.trim().to_string();
            if name.is_empty() || session.name == name {
                return Ok(None);
            }

            return Ok(Some(self.system_generated_name_update(id, name).await?));
        }

        let model_config = match session.model_config.clone() {
            Some(model_config) => model_config,
            None => {
                let model_name =
                    crate::config::Config::global()
                        .get_goose_model()
                        .map_err(|_| {
                            anyhow::anyhow!("Could not resolve model config: missing model")
                        })?;
                crate::model_config::model_config_from_user_config(
                    provider.get_name(),
                    &model_name,
                )?
            }
        };
        let conversation = session
            .conversation
            .ok_or_else(|| anyhow::anyhow!("No messages found"))?;

        let user_message_count = conversation
            .messages()
            .iter()
            .filter(|m| matches!(m.role, Role::User) && m.is_user_visible())
            .count();

        let should_generate_name = if provider.manages_own_context() {
            user_message_count == 1
        } else {
            user_message_count <= MSG_COUNT_FOR_SESSION_NAME_GENERATION
        };

        if should_generate_name {
            let name = generate_session_name(
                provider.as_ref(),
                &model_config,
                id,
                &conversation,
                Some(session.working_dir.as_path()),
            )
            .await?;
            return Ok(Some(self.system_generated_name_update(id, name).await?));
        }
        Ok(None)
    }

    pub async fn search_chat_history(
        &self,
        query: &str,
        limit: Option<usize>,
        after_date: Option<chrono::DateTime<chrono::Utc>>,
        before_date: Option<chrono::DateTime<chrono::Utc>>,
        exclude_session_id: Option<String>,
        session_types: Vec<SessionType>,
    ) -> Result<crate::session::chat_history_search::ChatRecallResults> {
        self.storage
            .search_chat_history(
                query,
                limit,
                after_date,
                before_date,
                exclude_session_id,
                session_types,
            )
            .await
    }

    pub async fn update_message_metadata<F>(&self, id: &str, message_id: &str, f: F) -> Result<()>
    where
        F: FnOnce(
            bcaip_provider_types::conversations::MessageMetadata,
        ) -> bcaip_provider_types::conversations::MessageMetadata,
    {
        self.storage
            .update_message_metadata(id, message_id, f)
            .await
    }

    /// Patch `tool_meta` on a specific `ToolRequest` within a stored message.
    /// Used to persist LLM-generated tool titles and chain summaries so they
    /// survive session reload. Merge-based: existing keys not in `patch` are
    /// preserved. Searches the most recently inserted messages in the session
    /// and is a no-op if the tool_call_id is not found.
    pub async fn update_tool_request_meta(
        &self,
        session_id: &str,
        tool_call_id: &str,
        patch: serde_json::Value,
    ) -> Result<()> {
        self.storage
            .update_tool_request_meta(session_id, tool_call_id, patch)
            .await
    }
}

pub struct SessionStorage {
    pool: Pool<Sqlite>,
    initialized: tokio::sync::OnceCell<()>,
    session_dir: PathBuf,
    action_required: Arc<crate::action_required_manager::ActionRequiredManager>,
}

pub(crate) fn role_to_string(role: &Role) -> &'static str {
    match role {
        Role::User => "user",
        Role::Assistant => "assistant",
    }
}

fn message_timestamp_to_datetime(timestamp: i64) -> Option<DateTime<Utc>> {
    let timestamp = if timestamp > MILLISECOND_TIMESTAMP_THRESHOLD {
        timestamp / 1000
    } else {
        timestamp
    };
    Utc.timestamp_opt(timestamp, 0).single()
}

fn normalized_message_timestamp_sql(column: &str) -> String {
    format!(
        "CASE WHEN {column} > {MILLISECOND_TIMESTAMP_THRESHOLD} THEN {column} / 1000 ELSE {column} END"
    )
}

fn user_visible_message_sql(column: &str) -> String {
    format!("COALESCE(json_extract({column}, '$.userVisible'), 1) != 0")
}

fn session_sort_at(session: &Session) -> DateTime<Utc> {
    session.last_message_at.unwrap_or(session.updated_at)
}

impl Default for Session {
    fn default() -> Self {
        Self {
            id: String::new(),
            working_dir: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            name: String::new(),
            user_set_name: false,
            session_type: SessionType::default(),
            created_at: Default::default(),
            updated_at: Default::default(),
            extension_data: ExtensionData::default(),
            usage: Usage::default(),
            accumulated_usage: Usage::default(),
            accumulated_cost: None,
            schedule_id: None,
            recipe: None,
            user_recipe_values: None,
            conversation: None,
            message_count: 0,
            last_message_at: None,
            provider_name: None,
            model_config: None,
            goose_mode: GooseMode::default(),
            archived_at: None,
            project_id: None,
            parent_session_id: None,
            last_message_snippet: None,
        }
    }
}

impl Session {
    pub fn without_messages(mut self) -> Self {
        self.conversation = None;
        self
    }
}

fn deserialize_session_model_config(
    provider_name: Option<&str>,
    json: &str,
) -> Option<ModelConfig> {
    let mut model_config: ModelConfig = serde_json::from_str(json).ok()?;
    // TODO: Remove this workaround once ModelConfig guarantees deserialize(serialize(config)) == config.
    if provider_name == Some(goose_providers::azure_foundry::AZURE_FOUNDRY_PROVIDER_NAME) {
        #[derive(Deserialize)]
        struct AzurePersistedFields {
            model_name: String,
            #[serde(default)]
            request_params: Option<HashMap<String, serde_json::Value>>,
        }

        let persisted: AzurePersistedFields = serde_json::from_str(json).ok()?;
        model_config.model_name = persisted.model_name;
        model_config.request_params = persisted.request_params;
    }
    Some(model_config)
}

impl sqlx::FromRow<'_, sqlx::sqlite::SqliteRow> for Session {
    fn from_row(row: &sqlx::sqlite::SqliteRow) -> Result<Self, sqlx::Error> {
        use sqlx::Row;
        let recipe_json: Option<String> = row.try_get("recipe_json")?;
        let recipe = recipe_json.and_then(|json| serde_json::from_str(&json).ok());

        let user_recipe_values_json: Option<String> = row.try_get("user_recipe_values_json")?;
        let user_recipe_values =
            user_recipe_values_json.and_then(|json| serde_json::from_str(&json).ok());

        let provider_name: Option<String> = row.try_get("provider_name").ok().flatten();
        let model_config_json: Option<String> = row.try_get("model_config_json").ok().flatten();
        let model_config = model_config_json
            .as_deref()
            .and_then(|json| deserialize_session_model_config(provider_name.as_deref(), json));

        let name: String = {
            let name_val: String = row.try_get("name").unwrap_or_default();
            if !name_val.is_empty() {
                name_val
            } else {
                row.try_get("description").unwrap_or_default()
            }
        };

        let user_set_name = row.try_get("user_set_name").unwrap_or(false);

        let session_type_str: String = row
            .try_get("session_type")
            .unwrap_or_else(|_| "user".to_string());
        let session_type = session_type_str.parse().unwrap_or_default();

        let last_message_at = row
            .try_get::<Option<i64>, _>("last_message_timestamp")
            .ok()
            .flatten()
            .and_then(message_timestamp_to_datetime);

        Ok(Session {
            id: row.try_get("id")?,
            working_dir: PathBuf::from(row.try_get::<String, _>("working_dir")?),
            name,
            user_set_name,
            session_type,
            created_at: row.try_get("created_at")?,
            updated_at: row.try_get("updated_at")?,
            extension_data: serde_json::from_str(&row.try_get::<String, _>("extension_data")?)
                .unwrap_or_default(),
            usage: Usage {
                input_tokens: row.try_get("input_tokens")?,
                output_tokens: row.try_get("output_tokens")?,
                total_tokens: row.try_get("total_tokens")?,
                cache_read_input_tokens: row.try_get("cache_read_tokens").ok().flatten(),
                cache_write_input_tokens: row.try_get("cache_write_tokens").ok().flatten(),
            },
            accumulated_usage: Usage {
                input_tokens: row.try_get("accumulated_input_tokens")?,
                output_tokens: row.try_get("accumulated_output_tokens")?,
                total_tokens: row.try_get("accumulated_total_tokens")?,
                cache_read_input_tokens: row
                    .try_get("accumulated_cache_read_tokens")
                    .ok()
                    .flatten(),
                cache_write_input_tokens: row
                    .try_get("accumulated_cache_write_tokens")
                    .ok()
                    .flatten(),
            },
            accumulated_cost: row.try_get("accumulated_cost").ok().flatten(),
            schedule_id: row.try_get("schedule_id")?,
            recipe,
            user_recipe_values,
            conversation: None,
            message_count: row.try_get("message_count").unwrap_or(0) as usize,
            last_message_at,
            provider_name,
            model_config,
            goose_mode: row
                .try_get::<String, _>("goose_mode")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or_default(),
            archived_at: row.try_get("archived_at").ok(),
            project_id: row.try_get("project_id").ok().flatten(),
            parent_session_id: row.try_get("parent_session_id").ok().flatten(),
            last_message_snippet: None,
        })
    }
}

async fn insert_usage_ledger_row(
    tx: &mut sqlx::Transaction<'_, Sqlite>,
    session_id: &str,
    model: Option<&str>,
    usage: &MessageUsage,
) -> Result<()> {
    let cost_source = usage.cost_source.map(|cs| match cs {
        CostSource::ProviderReported => "provider_reported",
        CostSource::Estimated => "estimated",
    });

    sqlx::query(
        r#"
        INSERT INTO usage_ledger (
            session_id, created_timestamp, model,
            input_tokens, output_tokens, total_tokens,
            cache_read_tokens, cache_write_tokens,
            cost, cost_source, is_compaction
        )
        VALUES (?, strftime('%s','now'), ?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(session_id)
    .bind(model)
    .bind(usage.input_tokens)
    .bind(usage.output_tokens)
    .bind(usage.total_tokens)
    .bind(usage.cache_read_tokens)
    .bind(usage.cache_write_tokens)
    .bind(usage.cost)
    .bind(cost_source)
    .bind(usage.is_compaction as i64)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

impl SessionStorage {
    fn create_pool(path: &Path) -> Pool<Sqlite> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("Failed to create session database directory");
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(parent, fs::Permissions::from_mode(0o700))
                    .expect("Failed to secure session database directory");
            }
        }

        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .foreign_keys(true)
            .busy_timeout(std::time::Duration::from_secs(30))
            .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal);

        SqlitePoolOptions::new().connect_lazy_with(options)
    }

    pub fn new(data_dir: PathBuf) -> Self {
        let session_dir = data_dir.join(SESSIONS_FOLDER);
        let db_path = session_dir.join(DB_NAME);
        Self {
            pool: Self::create_pool(&db_path),
            initialized: tokio::sync::OnceCell::new(),
            session_dir,
            action_required: Arc::new(crate::action_required_manager::ActionRequiredManager::new()),
        }
    }

    pub(crate) async fn pool(&self) -> Result<&Pool<Sqlite>> {
        self.initialized
            .get_or_try_init(|| async {
                let schema_exists = sqlx::query_scalar::<_, bool>(
                    r#"SELECT EXISTS (SELECT name FROM sqlite_master WHERE type='table' AND name='schema_version')"#,
                )
                .fetch_one(&self.pool)
                .await
                .unwrap_or(false);

                if schema_exists {
                    Self::run_migrations(&self.pool).await?;
                } else {
                    Self::create_schema(&self.pool).await?;
                    if let Err(e) = Self::import_legacy(&self.pool, &self.session_dir).await {
                        warn!("Failed to import some legacy sessions: {}", e);
                    }
                }
                Ok::<(), anyhow::Error>(())
            })
            .await?;
        Ok(&self.pool)
    }

    async fn create_schema(pool: &Pool<Sqlite>) -> Result<()> {
        // Run schema creation under `BEGIN IMMEDIATE` so SQLite serializes
        // writers across processes. Combined with `IF NOT EXISTS` on every
        // DDL statement and `INSERT OR IGNORE` on the bootstrap version
        // row, this makes init safe under concurrent first-run startup —
        // the previous flow:
        //
        //   SELECT EXISTS('schema_version') → false
        //   CREATE TABLE schema_version (...)
        //
        // raced when two processes both saw "doesn't exist" and the
        // second one's CREATE TABLE failed with `table already exists`,
        // which surfaced to callers as "Could not create session".
        let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS schema_version (
                version INTEGER PRIMARY KEY,
                applied_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
            )
        "#,
        )
        .execute(&mut *tx)
        .await?;

        sqlx::query("INSERT OR IGNORE INTO schema_version (version) VALUES (?)")
            .bind(CURRENT_SCHEMA_VERSION)
            .execute(&mut *tx)
            .await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS sessions (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL DEFAULT '',
                description TEXT NOT NULL DEFAULT '',
                user_set_name BOOLEAN DEFAULT FALSE,
                session_type TEXT NOT NULL DEFAULT 'user',
                working_dir TEXT NOT NULL,
                created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
                updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
                extension_data TEXT DEFAULT '{}',
                total_tokens INTEGER,
                input_tokens INTEGER,
                output_tokens INTEGER,
                cache_read_tokens INTEGER,
                cache_write_tokens INTEGER,
                accumulated_total_tokens INTEGER,
                accumulated_input_tokens INTEGER,
                accumulated_output_tokens INTEGER,
                accumulated_cache_read_tokens INTEGER,
                accumulated_cache_write_tokens INTEGER,
                accumulated_cost REAL,
                schedule_id TEXT,
                recipe_json TEXT,
                user_recipe_values_json TEXT,
                provider_name TEXT,
                model_config_json TEXT,
                goose_mode TEXT NOT NULL DEFAULT 'auto',
                archived_at TIMESTAMP,
                project_id TEXT,
                parent_session_id TEXT
            )
        "#,
        )
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS messages (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                message_id TEXT,
                session_id TEXT NOT NULL REFERENCES sessions(id),
                role TEXT NOT NULL,
                content_json TEXT NOT NULL,
                created_timestamp INTEGER NOT NULL,
                timestamp TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
                tokens INTEGER,
                metadata_json TEXT
            )
        "#,
        )
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS usage_ledger (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
                created_timestamp INTEGER NOT NULL,
                model TEXT,
                input_tokens INTEGER,
                output_tokens INTEGER,
                total_tokens INTEGER,
                cache_read_tokens INTEGER,
                cache_write_tokens INTEGER,
                cost REAL,
                cost_source TEXT,
                is_compaction INTEGER DEFAULT 0
            )
        "#,
        )
        .execute(&mut *tx)
        .await?;

        sqlx::query("CREATE INDEX IF NOT EXISTS idx_messages_session ON messages(session_id)")
            .execute(&mut *tx)
            .await?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_messages_timestamp ON messages(timestamp)")
            .execute(&mut *tx)
            .await?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_messages_message_id ON messages(message_id)")
            .execute(&mut *tx)
            .await?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_messages_session_created ON messages(session_id, created_timestamp, id)",
        )
        .execute(&mut *tx)
        .await?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_sessions_updated ON sessions(updated_at DESC)")
            .execute(&mut *tx)
            .await?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_sessions_type ON sessions(session_type)")
            .execute(&mut *tx)
            .await?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_sessions_parent ON sessions(parent_session_id)",
        )
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_usage_ledger_session ON usage_ledger(session_id)",
        )
        .execute(&mut *tx)
        .await?;

        // Create the inventory tables inside the same transaction so that a
        // second SessionStorage opening the same DB file never observes a
        // committed schema_version (and thus takes the migration path) while
        // the inventory tables don't yet exist — which raced as
        // `no such table: provider_inventory_entries`.
        crate::providers::inventory::create_tables(&mut tx).await?;

        tx.commit().await?;

        Ok(())
    }

    async fn import_legacy(pool: &Pool<Sqlite>, session_dir: &PathBuf) -> Result<()> {
        use crate::session::legacy;
        let sessions = match legacy::list_sessions(session_dir) {
            Ok(sessions) => sessions,
            Err(_) => {
                warn!("No legacy sessions found to import");
                return Ok(());
            }
        };

        if sessions.is_empty() {
            return Ok(());
        }

        let mut imported_count = 0;
        let mut failed_count = 0;

        for (session_name, session_path) in sessions {
            match legacy::load_session(&session_name, &session_path) {
                Ok(session) => match Self::import_legacy_session(pool, &session).await {
                    Ok(_) => {
                        imported_count += 1;
                        info!("  ✓ Imported: {}", session_name);
                    }
                    Err(e) => {
                        failed_count += 1;
                        info!("  ✗ Failed to import {}: {}", session_name, e);
                    }
                },
                Err(e) => {
                    failed_count += 1;
                    info!("  ✗ Failed to load {}: {}", session_name, e);
                }
            }
        }

        info!(
            "Import complete: {} successful, {} failed",
            imported_count, failed_count
        );
        Ok(())
    }

    async fn import_legacy_session(pool: &Pool<Sqlite>, session: &Session) -> Result<()> {
        let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;

        let recipe_json = match &session.recipe {
            Some(recipe) => Some(serde_json::to_string(&strip_unreferenced_parameters(
                recipe.clone(),
            ))?),
            None => None,
        };

        let user_recipe_values_json = match &session.user_recipe_values {
            Some(user_recipe_values) => Some(serde_json::to_string(user_recipe_values)?),
            None => None,
        };

        let model_config_json = match &session.model_config {
            Some(model_config) => Some(serde_json::to_string(model_config)?),
            None => None,
        };

        sqlx::query(
            r#"
        INSERT INTO sessions (
            id, name, user_set_name, session_type, working_dir, created_at, updated_at, extension_data,
            total_tokens, input_tokens, output_tokens,
            cache_read_tokens, cache_write_tokens,
            accumulated_total_tokens, accumulated_input_tokens, accumulated_output_tokens,
            accumulated_cache_read_tokens, accumulated_cache_write_tokens,
            accumulated_cost,
            schedule_id, recipe_json, user_recipe_values_json,
            provider_name, model_config_json, goose_mode
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
        )
        .bind(&session.id)
        .bind(&session.name)
        .bind(session.user_set_name)
        .bind(session.session_type.to_string())
        .bind(&*session.working_dir.to_string_lossy())
        .bind(session.created_at)
        .bind(session.updated_at)
        .bind(serde_json::to_string(&session.extension_data)?)
        .bind(session.usage.total_tokens)
        .bind(session.usage.input_tokens)
        .bind(session.usage.output_tokens)
        .bind(session.usage.cache_read_input_tokens)
        .bind(session.usage.cache_write_input_tokens)
        .bind(session.accumulated_usage.total_tokens)
        .bind(session.accumulated_usage.input_tokens)
        .bind(session.accumulated_usage.output_tokens)
        .bind(session.accumulated_usage.cache_read_input_tokens)
        .bind(session.accumulated_usage.cache_write_input_tokens)
        .bind(session.accumulated_cost)
        .bind(&session.schedule_id)
        .bind(recipe_json)
        .bind(user_recipe_values_json)
        .bind(&session.provider_name)
        .bind(model_config_json)
        .bind(session.goose_mode.to_string())
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;

        if let Some(conversation) = &session.conversation {
            Self::replace_conversation_inner(pool, &session.id, conversation).await?;
        }
        Ok(())
    }

    async fn run_migrations(pool: &Pool<Sqlite>) -> Result<()> {
        let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;

        let current_version = Self::get_schema_version(&mut tx).await?;

        if current_version < CURRENT_SCHEMA_VERSION {
            info!(
                "Running database migrations from v{} to v{}...",
                current_version, CURRENT_SCHEMA_VERSION
            );

            for version in (current_version + 1)..=CURRENT_SCHEMA_VERSION {
                info!("  Applying migration v{}...", version);
                Self::apply_migration(&mut tx, version).await?;
                Self::update_schema_version(&mut tx, version).await?;
                info!("  ✓ Migration v{} complete", version);
            }

            info!("All migrations complete");
        }

        tx.commit().await?;
        Ok(())
    }

    async fn get_schema_version(tx: &mut sqlx::Transaction<'_, Sqlite>) -> Result<i32> {
        let table_exists = sqlx::query_scalar::<_, bool>(
            r#"
            SELECT EXISTS (
                SELECT name FROM sqlite_master
                WHERE type='table' AND name='schema_version'
            )
        "#,
        )
        .fetch_one(&mut **tx)
        .await?;

        if !table_exists {
            return Ok(0);
        }

        let version = sqlx::query_scalar::<_, i32>("SELECT MAX(version) FROM schema_version")
            .fetch_one(&mut **tx)
            .await?;

        Ok(version)
    }

    async fn update_schema_version(
        tx: &mut sqlx::Transaction<'_, Sqlite>,
        version: i32,
    ) -> Result<()> {
        sqlx::query("INSERT INTO schema_version (version) VALUES (?)")
            .bind(version)
            .execute(&mut **tx)
            .await?;
        Ok(())
    }

    #[allow(clippy::too_many_lines)]
    async fn apply_migration(tx: &mut sqlx::Transaction<'_, Sqlite>, version: i32) -> Result<()> {
        match version {
            1 => {
                sqlx::query(
                    r#"
                    CREATE TABLE IF NOT EXISTS schema_version (
                        version INTEGER PRIMARY KEY,
                        applied_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
                    )
                "#,
                )
                .execute(&mut **tx)
                .await?;
            }
            2 => {
                sqlx::query(
                    r#"
                    ALTER TABLE sessions ADD COLUMN user_recipe_values_json TEXT
                "#,
                )
                .execute(&mut **tx)
                .await?;
            }
            3 => {
                sqlx::query(
                    r#"
                    ALTER TABLE messages ADD COLUMN metadata_json TEXT
                "#,
                )
                .execute(&mut **tx)
                .await?;
            }
            4 => {
                sqlx::query(
                    r#"
                    ALTER TABLE sessions ADD COLUMN name TEXT DEFAULT ''
                "#,
                )
                .execute(&mut **tx)
                .await?;

                sqlx::query(
                    r#"
                    ALTER TABLE sessions ADD COLUMN user_set_name BOOLEAN DEFAULT FALSE
                "#,
                )
                .execute(&mut **tx)
                .await?;
            }
            5 => {
                sqlx::query(
                    r#"
                    ALTER TABLE sessions ADD COLUMN session_type TEXT NOT NULL DEFAULT 'user'
                "#,
                )
                .execute(&mut **tx)
                .await?;

                sqlx::query("CREATE INDEX idx_sessions_type ON sessions(session_type)")
                    .execute(&mut **tx)
                    .await?;
            }
            6 => {
                sqlx::query(
                    r#"
                    ALTER TABLE sessions ADD COLUMN provider_name TEXT
                "#,
                )
                .execute(&mut **tx)
                .await?;

                sqlx::query(
                    r#"
                    ALTER TABLE sessions ADD COLUMN model_config_json TEXT
                "#,
                )
                .execute(&mut **tx)
                .await?;
            }
            7 => {
                sqlx::query(
                    r#"
                    ALTER TABLE messages ADD COLUMN message_id TEXT
                "#,
                )
                .execute(&mut **tx)
                .await?;

                sqlx::query(
                    r#"
                    UPDATE messages
                    SET message_id = 'msg_' || session_id || '_' || id
                "#,
                )
                .execute(&mut **tx)
                .await?;

                sqlx::query("CREATE INDEX idx_messages_message_id ON messages(message_id)")
                    .execute(&mut **tx)
                    .await?;
            }
            8 => {
                sqlx::query(
                    r#"
                    ALTER TABLE sessions ADD COLUMN goose_mode TEXT NOT NULL DEFAULT 'auto'
                "#,
                )
                .execute(&mut **tx)
                .await?;
            }
            9 => {
                sqlx::query(
                    r#"
                    UPDATE sessions
                    SET session_type = 'acp'
                    WHERE session_type = 'user'
                      AND name = 'ACP Session'
                      AND user_set_name = FALSE
                "#,
                )
                .execute(&mut **tx)
                .await?;
            }
            10 => {
                // Check if thread_id column already exists (e.g. fresh schema)
                let has_thread_id = sqlx::query_scalar::<_, i32>(
                    "SELECT COUNT(*) FROM pragma_table_info('sessions') WHERE name = 'thread_id'",
                )
                .fetch_one(&mut **tx)
                .await?
                    > 0;
                if !has_thread_id {
                    sqlx::query("ALTER TABLE sessions ADD COLUMN thread_id TEXT")
                        .execute(&mut **tx)
                        .await?;
                }
                sqlx::query(
                    "CREATE INDEX IF NOT EXISTS idx_sessions_thread ON sessions(thread_id)",
                )
                .execute(&mut **tx)
                .await?;
                sqlx::query(
                    "CREATE TABLE IF NOT EXISTS threads (
                        id TEXT PRIMARY KEY,
                        name TEXT NOT NULL DEFAULT 'New Chat',
                        user_set_name BOOLEAN DEFAULT FALSE,
                        working_dir TEXT,
                        created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
                        updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
                        archived_at TIMESTAMP,
                        metadata_json TEXT DEFAULT '{}'
                    )",
                )
                .execute(&mut **tx)
                .await?;
                sqlx::query(
                    "CREATE TABLE IF NOT EXISTS thread_messages (
                        id INTEGER PRIMARY KEY AUTOINCREMENT,
                        thread_id TEXT NOT NULL REFERENCES threads(id),
                        session_id TEXT,
                        message_id TEXT,
                        role TEXT NOT NULL,
                        content_json TEXT NOT NULL,
                        created_timestamp INTEGER NOT NULL,
                        metadata_json TEXT DEFAULT '{}'
                    )",
                )
                .execute(&mut **tx)
                .await?;
                sqlx::query("CREATE INDEX IF NOT EXISTS idx_thread_messages_thread ON thread_messages(thread_id)")
                    .execute(&mut **tx)
                    .await?;
                sqlx::query("CREATE INDEX IF NOT EXISTS idx_thread_messages_message_id ON thread_messages(message_id)")
                    .execute(&mut **tx)
                    .await?;
            }
            11 => {
                crate::providers::inventory::create_tables(tx).await?;
            }
            12 => {
                // Add archived_at, project_id columns to sessions.
                let has_archived_at = sqlx::query_scalar::<_, i32>(
                    "SELECT COUNT(*) FROM pragma_table_info('sessions') WHERE name = 'archived_at'",
                )
                .fetch_one(&mut **tx)
                .await?
                    > 0;
                if !has_archived_at {
                    sqlx::query("ALTER TABLE sessions ADD COLUMN archived_at TIMESTAMP")
                        .execute(&mut **tx)
                        .await?;
                }

                let has_project_id = sqlx::query_scalar::<_, i32>(
                    "SELECT COUNT(*) FROM pragma_table_info('sessions') WHERE name = 'project_id'",
                )
                .fetch_one(&mut **tx)
                .await?
                    > 0;
                if !has_project_id {
                    sqlx::query("ALTER TABLE sessions ADD COLUMN project_id TEXT")
                        .execute(&mut **tx)
                        .await?;
                }
            }
            13 => {
                let has_accumulated_cost = sqlx::query_scalar::<_, i32>(
                    "SELECT COUNT(*) FROM pragma_table_info('sessions') WHERE name = 'accumulated_cost'",
                )
                .fetch_one(&mut **tx)
                .await?
                    > 0;
                if !has_accumulated_cost {
                    sqlx::query("ALTER TABLE sessions ADD COLUMN accumulated_cost REAL")
                        .execute(&mut **tx)
                        .await?;
                }
            }
            14 => {
                for column in [
                    "cache_read_tokens",
                    "cache_write_tokens",
                    "accumulated_cache_read_tokens",
                    "accumulated_cache_write_tokens",
                ] {
                    let has_column = sqlx::query_scalar::<_, i32>(
                        "SELECT COUNT(*) FROM pragma_table_info('sessions') WHERE name = ?",
                    )
                    .bind(column)
                    .fetch_one(&mut **tx)
                    .await?
                        > 0;
                    if !has_column {
                        sqlx::query(AssertSqlSafe(format!(
                            "ALTER TABLE sessions ADD COLUMN {column} INTEGER"
                        )))
                        .execute(&mut **tx)
                        .await?;
                    }
                }
            }
            15 => {
                let has_parent = sqlx::query_scalar::<_, i32>(
                    "SELECT COUNT(*) FROM pragma_table_info('sessions') WHERE name = 'parent_session_id'",
                )
                .fetch_one(&mut **tx)
                .await?
                    > 0;
                if !has_parent {
                    sqlx::query("ALTER TABLE sessions ADD COLUMN parent_session_id TEXT")
                        .execute(&mut **tx)
                        .await?;
                    sqlx::query(
                        "CREATE INDEX IF NOT EXISTS idx_sessions_parent ON sessions(parent_session_id)",
                    )
                    .execute(&mut **tx)
                    .await?;
                }

                sqlx::query(
                    r#"
                    CREATE TABLE IF NOT EXISTS usage_ledger (
                        id INTEGER PRIMARY KEY AUTOINCREMENT,
                        session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
                        created_timestamp INTEGER NOT NULL,
                        model TEXT,
                        input_tokens INTEGER,
                        output_tokens INTEGER,
                        total_tokens INTEGER,
                        cache_read_tokens INTEGER,
                        cache_write_tokens INTEGER,
                        cost REAL,
                        cost_source TEXT,
                        is_compaction INTEGER DEFAULT 0
                    )
                    "#,
                )
                .execute(&mut **tx)
                .await?;
                sqlx::query(
                    "CREATE INDEX IF NOT EXISTS idx_usage_ledger_session ON usage_ledger(session_id)",
                )
                .execute(&mut **tx)
                .await?;
            }
            16 => {
                sqlx::query(
                    "CREATE INDEX IF NOT EXISTS idx_messages_session_created ON messages(session_id, created_timestamp, id)",
                )
                .execute(&mut **tx)
                .await?;
            }
            _ => {
                anyhow::bail!("Unknown migration version: {}", version);
            }
        }

        Ok(())
    }

    async fn create_session(
        &self,
        working_dir: PathBuf,
        name: String,
        session_type: SessionType,
        goose_mode: GooseMode,
    ) -> Result<Session> {
        let pool = self.pool().await?;
        let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;

        let today = chrono::Utc::now().format("%Y%m%d").to_string();
        let session = sqlx::query_as(
            r#"
                INSERT INTO sessions (id, name, user_set_name, session_type, working_dir, extension_data, goose_mode)
                VALUES (
                    ? || '_' || CAST(COALESCE((
                        SELECT MAX(CAST(SUBSTR(id, 10) AS INTEGER))
                        FROM sessions
                        WHERE id LIKE ? || '_%'
                    ), 0) + 1 AS TEXT),
                    ?,
                    FALSE,
                    ?,
                    ?,
                    '{}',
                    ?
                )
                RETURNING *
                "#,
        )
            .bind(&today)
            .bind(&today)
            .bind(&name)
            .bind(session_type.to_string())
            .bind(&*working_dir.to_string_lossy())
            .bind(goose_mode.to_string())
            .fetch_one(&mut *tx)
            .await?;

        tx.commit().await?;
        #[cfg(feature = "telemetry")]
        crate::posthog::emit_session_started();
        Ok(session)
    }

    async fn get_session(&self, id: &str, include_messages: bool) -> Result<Session> {
        let pool = self.pool().await?;
        let mut session = sqlx::query_as::<_, Session>(
            r#"
        SELECT id, working_dir, name, description, user_set_name, session_type, created_at, updated_at, extension_data,
               total_tokens, input_tokens, output_tokens,
               cache_read_tokens, cache_write_tokens,
               accumulated_total_tokens, accumulated_input_tokens, accumulated_output_tokens,
               accumulated_cache_read_tokens, accumulated_cache_write_tokens,
               accumulated_cost,
               schedule_id, recipe_json, user_recipe_values_json,
               provider_name, model_config_json, goose_mode,
               archived_at, project_id, parent_session_id
        FROM sessions
        WHERE id = ?
    "#,
        )
            .bind(id)
            .fetch_optional(pool)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Session not found"))?;

        if include_messages {
            let conv = self.get_conversation(&session.id).await?;
            session.message_count = conv
                .messages()
                .iter()
                .filter(|m| m.is_user_visible())
                .count();
            session.last_message_at = conv
                .messages()
                .iter()
                .filter_map(|message| message_timestamp_to_datetime(message.created))
                .max();
            session.conversation = Some(conv);
        } else {
            let sql = format!(
                "SELECT COUNT(*) FILTER (WHERE {}), MAX({}) FROM messages WHERE session_id = ?",
                user_visible_message_sql("metadata_json"),
                normalized_message_timestamp_sql("created_timestamp")
            );
            let (count, last_message_timestamp): (i64, Option<i64>) =
                sqlx::query_as(AssertSqlSafe(sql))
                    .bind(&session.id)
                    .fetch_one(pool)
                    .await?;
            session.message_count = count as usize;
            session.last_message_at =
                last_message_timestamp.and_then(message_timestamp_to_datetime);
        }

        Ok(session)
    }

    #[allow(clippy::too_many_lines)]
    async fn apply_update(&self, builder: SessionUpdateBuilder<'_>) -> Result<()> {
        let mut updates = Vec::new();
        let mut query = String::from("UPDATE sessions SET ");

        macro_rules! add_update {
            ($field:expr, $name:expr) => {
                if $field.is_some() {
                    if !updates.is_empty() {
                        query.push_str(", ");
                    }
                    updates.push($name);
                    query.push_str($name);
                    query.push_str(" = ?");
                }
            };
        }

        add_update!(builder.name, "name");
        add_update!(builder.user_set_name, "user_set_name");
        add_update!(builder.session_type, "session_type");
        add_update!(builder.working_dir, "working_dir");
        add_update!(builder.extension_data, "extension_data");
        add_update!(builder.usage, "total_tokens");
        add_update!(builder.usage, "input_tokens");
        add_update!(builder.usage, "output_tokens");
        add_update!(builder.usage, "cache_read_tokens");
        add_update!(builder.usage, "cache_write_tokens");
        add_update!(builder.accumulated_usage, "accumulated_total_tokens");
        add_update!(builder.accumulated_usage, "accumulated_input_tokens");
        add_update!(builder.accumulated_usage, "accumulated_output_tokens");
        add_update!(builder.accumulated_usage, "accumulated_cache_read_tokens");
        add_update!(builder.accumulated_usage, "accumulated_cache_write_tokens");
        add_update!(builder.accumulated_cost, "accumulated_cost");
        add_update!(builder.schedule_id, "schedule_id");
        add_update!(builder.recipe, "recipe_json");
        add_update!(builder.user_recipe_values, "user_recipe_values_json");
        add_update!(builder.provider_name, "provider_name");
        add_update!(builder.model_config, "model_config_json");
        add_update!(builder.goose_mode, "goose_mode");
        add_update!(builder.archived_at, "archived_at");

        add_update!(builder.project_id, "project_id");
        add_update!(builder.parent_session_id, "parent_session_id");

        if updates.is_empty() {
            return Ok(());
        }

        query.push_str(", ");
        query.push_str("updated_at = datetime('now') WHERE id = ?");

        let mut q = sqlx::query(AssertSqlSafe(query));

        if let Some(name) = builder.name {
            q = q.bind(name);
        }
        if let Some(user_set_name) = builder.user_set_name {
            q = q.bind(user_set_name);
        }
        if let Some(session_type) = builder.session_type {
            q = q.bind(session_type.to_string());
        }
        if let Some(wd) = builder.working_dir {
            q = q.bind(wd.to_string_lossy().to_string());
        }
        if let Some(ed) = builder.extension_data {
            q = q.bind(serde_json::to_string(&ed)?);
        }
        if let Some(u) = builder.usage {
            q = q
                .bind(u.total_tokens)
                .bind(u.input_tokens)
                .bind(u.output_tokens)
                .bind(u.cache_read_input_tokens)
                .bind(u.cache_write_input_tokens);
        }
        if let Some(u) = builder.accumulated_usage {
            q = q
                .bind(u.total_tokens)
                .bind(u.input_tokens)
                .bind(u.output_tokens)
                .bind(u.cache_read_input_tokens)
                .bind(u.cache_write_input_tokens);
        }
        if let Some(ac) = builder.accumulated_cost {
            q = q.bind(ac);
        }
        if let Some(sid) = builder.schedule_id {
            q = q.bind(sid);
        }
        if let Some(recipe) = builder.recipe {
            let recipe_json = recipe.map(|r| serde_json::to_string(&r)).transpose()?;
            q = q.bind(recipe_json);
        }
        if let Some(user_recipe_values) = builder.user_recipe_values {
            let user_recipe_values_json = user_recipe_values
                .map(|urv| serde_json::to_string(&urv))
                .transpose()?;
            q = q.bind(user_recipe_values_json);
        }
        if let Some(provider_name) = builder.provider_name {
            q = q.bind(provider_name);
        }
        if let Some(model_config) = builder.model_config {
            let model_config_json = model_config
                .map(|mc| serde_json::to_string(&mc))
                .transpose()?;
            q = q.bind(model_config_json);
        }
        if let Some(goose_mode) = builder.goose_mode {
            q = q.bind(goose_mode.to_string());
        }
        if let Some(ref archived_at) = builder.archived_at {
            q = q.bind(archived_at.as_ref());
        }

        if let Some(ref project_id) = builder.project_id {
            q = q.bind(project_id.as_ref());
        }
        if let Some(ref parent_session_id) = builder.parent_session_id {
            q = q.bind(parent_session_id.as_ref());
        }

        let pool = self.pool().await?;
        let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
        q = q.bind(&builder.session_id);
        let result = q.execute(&mut *tx).await?;

        if result.rows_affected() == 0 {
            return Err(anyhow::anyhow!("Session not found: {}", builder.session_id));
        }

        tx.commit().await?;
        Ok(())
    }

    async fn update_project_for_session_types(
        &self,
        id: &str,
        project_id: Option<String>,
        session_types: &[SessionType],
    ) -> Result<bool> {
        if session_types.is_empty() {
            return Ok(false);
        }

        let placeholders = session_types
            .iter()
            .map(|_| "?")
            .collect::<Vec<_>>()
            .join(", ");
        let query = format!(
            "UPDATE sessions SET project_id = ?, updated_at = datetime('now') \
             WHERE id = ? AND session_type IN ({placeholders})"
        );
        let pool = self.pool().await?;
        let mut query = sqlx::query(AssertSqlSafe(query)).bind(project_id).bind(id);
        for session_type in session_types {
            query = query.bind(session_type.to_string());
        }

        Ok(query.execute(pool).await?.rows_affected() == 1)
    }

    async fn get_conversation(&self, session_id: &str) -> Result<Conversation> {
        let pool = self.pool().await?;
        let rows = sqlx::query_as::<_, (String, String, i64, Option<String>, Option<String>)>(
            // Order by created_timestamp, then by id to break ties. created_timestamp is in seconds,
            // so messages created in the same second (e.g., tool request and response) need to
            // maintain their insertion order via the auto-increment id.
            "SELECT role, content_json, created_timestamp, metadata_json, message_id FROM messages WHERE session_id = ? ORDER BY created_timestamp, id",
        )
            .bind(session_id)
            .fetch_all(pool)
            .await?;

        let mut messages = Vec::new();
        for (role_str, content_json, created_timestamp, metadata_json, message_id) in
            rows.into_iter()
        {
            let role = match role_str.as_str() {
                "user" => Role::User,
                "assistant" => Role::Assistant,
                _ => continue,
            };

            let content = serde_json::from_str(&content_json)?;
            let metadata = metadata_json
                .and_then(|json| serde_json::from_str(&json).ok())
                .unwrap_or_default();

            let mut message = Message::new(role, created_timestamp, content);
            message.metadata = metadata;
            if let Some(id) = message_id {
                message = message.with_id(id);
            }
            messages.push(message);
        }

        Ok(Conversation::new_unvalidated(messages))
    }

    async fn add_message(&self, session_id: &str, message: &Message) -> Result<()> {
        let pool = self.pool().await?;
        let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;

        let metadata_json = serde_json::to_string(&message.metadata)?;
        // Messages are read back ordered by (created_timestamp, id), so one built
        // before the messages it is appended after would sort ahead of them —
        // operations do that whenever they prepare a reply and fill it in while a
        // tool runs. Never move a message ahead of what is already stored.
        let latest: Option<i64> =
            sqlx::query_scalar("SELECT MAX(created_timestamp) FROM messages WHERE session_id = ?")
                .bind(session_id)
                .fetch_one(&mut *tx)
                .await?;
        let created = message.created.max(latest.unwrap_or(message.created));

        let message_id = message
            .id
            .clone()
            .unwrap_or_else(|| format!("msg_{}_{}", session_id, uuid::Uuid::new_v4()));

        sqlx::query(
            r#"
            INSERT INTO messages (message_id, session_id, role, content_json, created_timestamp, metadata_json)
            VALUES (?, ?, ?, ?, ?, ?)
        "#,
        )
        .bind(message_id)
        .bind(session_id)
        .bind(role_to_string(&message.role))
        .bind(serde_json::to_string(&message.content)?)
        .bind(created)
        .bind(metadata_json)
        .execute(&mut *tx)
        .await?;

        sqlx::query("UPDATE sessions SET updated_at = datetime('now') WHERE id = ?")
            .bind(session_id)
            .execute(&mut *tx)
            .await?;

        tx.commit().await?;
        Ok(())
    }

    async fn replace_conversation_inner(
        pool: &Pool<Sqlite>,
        session_id: &str,
        conversation: &Conversation,
    ) -> Result<()> {
        let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;

        sqlx::query("DELETE FROM messages WHERE session_id = ?")
            .bind(session_id)
            .execute(&mut *tx)
            .await?;

        for message in conversation.messages() {
            let metadata_json = serde_json::to_string(&message.metadata)?;

            let message_id = message
                .id
                .clone()
                .unwrap_or_else(|| format!("msg_{}_{}", session_id, uuid::Uuid::new_v4()));

            sqlx::query(
                r#"
            INSERT INTO messages (message_id, session_id, role, content_json, created_timestamp, metadata_json)
            VALUES (?, ?, ?, ?, ?, ?)
        "#,
            )
            .bind(message_id)
            .bind(session_id)
            .bind(role_to_string(&message.role))
            .bind(serde_json::to_string(&message.content)?)
            .bind(message.created)
            .bind(metadata_json)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(())
    }

    pub async fn replace_conversation(
        &self,
        session_id: &str,
        conversation: &Conversation,
    ) -> Result<()> {
        let pool = self.pool().await?;
        Self::replace_conversation_inner(pool, session_id, conversation).await
    }

    async fn save_compacted_conversation(
        &self,
        session_id: &str,
        conversation: &Conversation,
    ) -> Result<()> {
        let pool = self.pool().await?;
        let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;

        for message in conversation.messages() {
            let message_id = message
                .id
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("compacted conversation message has no id"))?;
            let stored_metadata_json = sqlx::query_scalar::<_, Option<String>>(
                "SELECT metadata_json FROM messages WHERE session_id = ? AND message_id = ?",
            )
            .bind(session_id)
            .bind(message_id)
            .fetch_optional(&mut *tx)
            .await?;
            if let Some(stored_metadata_json) = stored_metadata_json {
                let mut metadata = stored_metadata_json
                    .and_then(|json| serde_json::from_str::<MessageMetadata>(&json).ok())
                    .unwrap_or_default();
                metadata.agent_visible = message.metadata.agent_visible;
                sqlx::query(
                    "UPDATE messages SET metadata_json = ? WHERE session_id = ? AND message_id = ?",
                )
                .bind(serde_json::to_string(&metadata)?)
                .bind(session_id)
                .bind(message_id)
                .execute(&mut *tx)
                .await?;
            } else {
                sqlx::query(
                    "INSERT INTO messages (message_id, session_id, role, content_json, created_timestamp, metadata_json) VALUES (?, ?, ?, ?, ?, ?)",
                )
                .bind(message_id)
                .bind(session_id)
                .bind(role_to_string(&message.role))
                .bind(serde_json::to_string(&message.content)?)
                .bind(message.created)
                .bind(serde_json::to_string(&message.metadata)?)
                .execute(&mut *tx)
                .await?;
            }
        }

        sqlx::query("UPDATE sessions SET updated_at = datetime('now') WHERE id = ?")
            .bind(session_id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    async fn list_sessions_matching(&self, query: SessionListQuery<'_>) -> Result<Vec<Session>> {
        let filters = &query.filters;
        if matches!(filters.types, Some(types) if types.is_empty()) {
            return Ok(Vec::new());
        }

        let has_limit = query.limit.is_some();
        let keywords = keyword_terms(filters.keyword);
        let mut where_clauses = Vec::new();
        let mut having_clauses = Vec::new();
        let normalized_message_timestamp = normalized_message_timestamp_sql("m.created_timestamp");
        let sort_timestamp_sql =
            format!("COALESCE(MAX({normalized_message_timestamp}), unixepoch(s.updated_at))");
        if let Some(types) = filters.types {
            let placeholders = types.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
            where_clauses.push(format!("s.session_type IN ({})", placeholders));
        }
        if filters.working_dir.is_some() {
            where_clauses.push("s.working_dir = ?".to_string());
        }
        if !keywords.is_empty() {
            where_clauses.push(message_keyword_clause(keywords.len()));
        }
        if query.cursor.is_some() {
            having_clauses.push(format!(
                "({sort_timestamp_sql} < ? OR ({sort_timestamp_sql} = ? AND s.id < ?))"
            ));
        }

        let where_clause = if where_clauses.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", where_clauses.join(" AND "))
        };
        let having_clause = if having_clauses.is_empty() {
            String::new()
        } else {
            format!("HAVING {}", having_clauses.join(" AND "))
        };
        let message_join = if filters.only_sessions_with_messages {
            "JOIN messages m ON s.id = m.session_id"
        } else {
            "LEFT JOIN messages m ON s.id = m.session_id"
        };
        let order_by = "ORDER BY sort_timestamp DESC, s.id DESC";
        let limit_clause = if query.limit.is_some() { "LIMIT ?" } else { "" };

        let message_count_sql = if has_limit {
            "0".to_string()
        } else {
            format!(
                "COUNT(m.id) FILTER (WHERE {})",
                user_visible_message_sql("m.metadata_json")
            )
        };
        let sql = format!(
            r#"
            SELECT s.id, s.working_dir, s.name, s.description, s.user_set_name, s.session_type, s.created_at, s.updated_at, s.extension_data,
                   s.total_tokens, s.input_tokens, s.output_tokens,
                   s.cache_read_tokens, s.cache_write_tokens,
                   s.accumulated_total_tokens, s.accumulated_input_tokens, s.accumulated_output_tokens,
                   s.accumulated_cache_read_tokens, s.accumulated_cache_write_tokens,
                   s.accumulated_cost,
                   s.schedule_id, s.recipe_json, s.user_recipe_values_json,
                   s.provider_name, s.model_config_json, s.goose_mode,
                   s.archived_at, s.project_id, s.parent_session_id,
                   {} as message_count,
                   MAX({}) as last_message_timestamp,
                   {} as sort_timestamp
            FROM sessions s
            {}
            {}
            GROUP BY s.id
            {}
            {}
            {}
            "#,
            message_count_sql,
            normalized_message_timestamp,
            sort_timestamp_sql,
            message_join,
            where_clause,
            having_clause,
            order_by,
            limit_clause
        );

        let mut q = sqlx::query_as::<_, Session>(AssertSqlSafe(sql));
        if let Some(types) = filters.types {
            for session_type in types {
                q = q.bind(session_type.to_string());
            }
        }
        if let Some(working_dir) = filters.working_dir {
            q = q.bind(working_dir.to_string_lossy().to_string());
        }
        for term in keywords {
            q = q.bind(term);
        }
        if let Some(cursor) = query.cursor {
            let sort_at = cursor.sort_at.timestamp();
            q = q.bind(sort_at);
            q = q.bind(sort_at);
            q = q.bind(&cursor.session_id);
        }
        if let Some(limit) = query.limit {
            q = q.bind(limit as i64);
        }

        let pool = self.pool().await?;
        if has_limit {
            let mut tx = pool.begin().await?;
            let mut sessions = q.fetch_all(&mut *tx).await?;
            Self::populate_visible_message_counts(&mut tx, &mut sessions).await?;
            tx.commit().await?;
            Ok(sessions)
        } else {
            q.fetch_all(pool).await.map_err(Into::into)
        }
    }

    async fn populate_visible_message_counts(
        tx: &mut sqlx::Transaction<'_, Sqlite>,
        sessions: &mut [Session],
    ) -> Result<()> {
        let mut counts = HashMap::with_capacity(sessions.len());

        for chunk in sessions.chunks(SESSION_COUNT_BATCH_SIZE) {
            let placeholders = chunk.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
            let sql = format!(
                r#"
                SELECT m.session_id,
                       COUNT(m.id) FILTER (WHERE {}) as message_count
                FROM messages m
                WHERE m.session_id IN ({})
                GROUP BY m.session_id
                "#,
                user_visible_message_sql("m.metadata_json"),
                placeholders
            );
            let mut q = sqlx::query_as::<_, (String, i64)>(AssertSqlSafe(sql));
            for session in chunk {
                q = q.bind(&session.id);
            }
            for (session_id, message_count) in q.fetch_all(&mut **tx).await? {
                counts.insert(session_id, message_count as usize);
            }
        }

        for session in sessions {
            session.message_count = counts.get(&session.id).copied().unwrap_or_default();
        }

        Ok(())
    }

    async fn list_sessions_by_types(&self, types: Option<&[SessionType]>) -> Result<Vec<Session>> {
        self.list_sessions_matching(SessionListQuery {
            filters: SessionListFilters {
                types,
                ..Default::default()
            },
            ..Default::default()
        })
        .await
    }

    async fn list_sessions_paged(
        &self,
        query: SessionListPageQuery<'_>,
    ) -> Result<SessionListPage> {
        if matches!(query.filters.types, Some(types) if types.is_empty()) || query.page_size == 0 {
            return Ok(SessionListPage {
                sessions: Vec::new(),
                next_cursor: None,
            });
        }

        let page_size = query.page_size;
        let include_last_message_snippet = query.include_last_message_snippet;
        let mut sessions = self
            .list_sessions_matching(SessionListQuery {
                filters: query.filters,
                cursor: query.cursor,
                limit: Some(page_size + 1),
            })
            .await?;
        let has_next_page = sessions.len() > page_size;
        let next_cursor = if has_next_page {
            let anchor = &sessions[page_size - 1];
            Some(SessionListCursor {
                sort_at: session_sort_at(anchor),
                session_id: anchor.id.clone(),
            })
        } else {
            None
        };
        if has_next_page {
            sessions.truncate(page_size);
        }
        if include_last_message_snippet {
            let pool = self.pool().await?;
            super::last_message_snippet::hydrate_last_message_snippets(pool, &mut sessions).await?;
        }

        Ok(SessionListPage {
            sessions,
            next_cursor,
        })
    }

    async fn list_sessions(&self) -> Result<Vec<Session>> {
        self.list_sessions_by_types(Some(&[SessionType::User, SessionType::Scheduled]))
            .await
    }

    async fn delete_session(&self, session_id: &str) -> Result<()> {
        let pool = self.pool().await?;
        let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;

        let exists =
            sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM sessions WHERE id = ?)")
                .bind(session_id)
                .fetch_one(&mut *tx)
                .await?;

        if !exists {
            return Err(anyhow::anyhow!("Session not found"));
        }

        sqlx::query("DELETE FROM messages WHERE session_id = ?")
            .bind(session_id)
            .execute(&mut *tx)
            .await?;

        sqlx::query("DELETE FROM usage_ledger WHERE session_id = ?")
            .bind(session_id)
            .execute(&mut *tx)
            .await?;

        sqlx::query("DELETE FROM sessions WHERE id = ?")
            .bind(session_id)
            .execute(&mut *tx)
            .await?;

        tx.commit().await?;
        Ok(())
    }

    async fn get_insights(&self, types: &[SessionType]) -> Result<SessionInsights> {
        if types.is_empty() {
            return Ok(SessionInsights {
                total_sessions: 0,
                total_tokens: 0,
            });
        }

        let placeholders: String = types.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
        let query = format!(
            r#"
            SELECT COUNT(*) as total_sessions,
                   COALESCE(SUM(COALESCE(accumulated_total_tokens, total_tokens, 0)), 0) as total_tokens
            FROM sessions
            WHERE session_type IN ({})
            "#,
            placeholders
        );

        let pool = self.pool().await?;
        let mut q = sqlx::query_as::<_, (i64, Option<i64>)>(AssertSqlSafe(query));
        for t in types {
            q = q.bind(t.to_string());
        }

        let row = q.fetch_one(pool).await?;

        Ok(SessionInsights {
            total_sessions: row.0 as usize,
            total_tokens: row.1.unwrap_or(0),
        })
    }

    async fn record_usage_metrics(
        &self,
        session_id: &str,
        schedule_id: Option<String>,
        current_usage: Usage,
        model: &str,
        ledger: &MessageUsage,
    ) -> Result<()> {
        let pool = self.pool().await?;
        let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;

        sqlx::query(
            r#"
            INSERT INTO usage_ledger (
                session_id, created_timestamp,
                input_tokens, output_tokens, total_tokens,
                cache_read_tokens, cache_write_tokens,
                cost, cost_source
            )
            SELECT s.id, strftime('%s','now'),
                   MAX(COALESCE(s.accumulated_input_tokens, 0) - l.input_sum, 0),
                   MAX(COALESCE(s.accumulated_output_tokens, 0) - l.output_sum, 0),
                   MAX(COALESCE(s.accumulated_total_tokens, 0) - l.total_sum, 0),
                   MAX(COALESCE(s.accumulated_cache_read_tokens, 0) - l.cache_read_sum, 0),
                   MAX(COALESCE(s.accumulated_cache_write_tokens, 0) - l.cache_write_sum, 0),
                   CASE WHEN s.accumulated_cost IS NULL OR s.accumulated_cost <= l.cost_sum THEN NULL
                        ELSE s.accumulated_cost - l.cost_sum END,
                   'carried_forward'
            FROM sessions s,
                 (SELECT COALESCE(SUM(input_tokens), 0) AS input_sum,
                         COALESCE(SUM(output_tokens), 0) AS output_sum,
                         COALESCE(SUM(total_tokens), 0) AS total_sum,
                         COALESCE(SUM(cache_read_tokens), 0) AS cache_read_sum,
                         COALESCE(SUM(cache_write_tokens), 0) AS cache_write_sum,
                         COALESCE(SUM(cost), 0.0) AS cost_sum
                  FROM usage_ledger WHERE session_id = ?) l
            WHERE s.id = ?
              AND (COALESCE(s.accumulated_input_tokens, 0) > l.input_sum
                   OR COALESCE(s.accumulated_output_tokens, 0) > l.output_sum
                   OR COALESCE(s.accumulated_total_tokens, 0) > l.total_sum
                   OR COALESCE(s.accumulated_cost, 0.0) > l.cost_sum + 1e-9)
            "#,
        )
        .bind(session_id)
        .bind(session_id)
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            r#"
            UPDATE sessions SET
                schedule_id = ?,
                total_tokens = ?, input_tokens = ?, output_tokens = ?,
                cache_read_tokens = ?, cache_write_tokens = ?,
                accumulated_total_tokens = COALESCE(accumulated_total_tokens, 0) + ?,
                accumulated_input_tokens = COALESCE(accumulated_input_tokens, 0) + ?,
                accumulated_output_tokens = COALESCE(accumulated_output_tokens, 0) + ?,
                accumulated_cache_read_tokens = COALESCE(accumulated_cache_read_tokens, 0) + ?,
                accumulated_cache_write_tokens = COALESCE(accumulated_cache_write_tokens, 0) + ?,
                accumulated_cost = CASE
                    WHEN ? IS NULL THEN accumulated_cost
                    ELSE COALESCE(accumulated_cost, 0) + ?
                END,
                updated_at = datetime('now')
            WHERE id = ?
            "#,
        )
        .bind(schedule_id)
        .bind(current_usage.total_tokens)
        .bind(current_usage.input_tokens)
        .bind(current_usage.output_tokens)
        .bind(current_usage.cache_read_input_tokens)
        .bind(current_usage.cache_write_input_tokens)
        .bind(ledger.total_tokens.unwrap_or(0))
        .bind(ledger.input_tokens.unwrap_or(0))
        .bind(ledger.output_tokens.unwrap_or(0))
        .bind(ledger.cache_read_tokens.unwrap_or(0))
        .bind(ledger.cache_write_tokens.unwrap_or(0))
        .bind(ledger.cost)
        .bind(ledger.cost)
        .bind(session_id)
        .execute(&mut *tx)
        .await?;

        insert_usage_ledger_row(&mut tx, session_id, Some(model), ledger).await?;

        tx.commit().await?;
        Ok(())
    }

    async fn get_session_usage_totals(&self, session_id: &str) -> Result<SessionUsageTotals> {
        let pool = self.pool().await?;
        let rows = sqlx::query_as::<
            _,
            (
                Option<i64>,
                Option<i64>,
                Option<i64>,
                Option<i64>,
                Option<i64>,
                Option<f64>,
                Option<i64>,
                Option<i64>,
                Option<i64>,
                Option<i64>,
                Option<i64>,
                Option<f64>,
            ),
        >(
            r#"
            WITH RECURSIVE tree(id) AS (
                SELECT id FROM sessions WHERE id = ?
                UNION
                SELECT s.id FROM sessions s JOIN tree ON s.parent_session_id = tree.id
            )
            SELECT
                s.accumulated_input_tokens, s.accumulated_output_tokens, s.accumulated_total_tokens,
                s.accumulated_cache_read_tokens, s.accumulated_cache_write_tokens, s.accumulated_cost,
                SUM(u.input_tokens), SUM(u.output_tokens), SUM(u.total_tokens),
                SUM(u.cache_read_tokens), SUM(u.cache_write_tokens), SUM(u.cost)
            FROM sessions s
            LEFT JOIN usage_ledger u ON u.session_id = s.id
            WHERE s.id IN (SELECT id FROM tree)
            GROUP BY s.id
            "#,
        )
        .bind(session_id)
        .fetch_all(pool)
        .await?;

        let mut input = 0i64;
        let mut output = 0i64;
        let mut total = 0i64;
        let mut cache_read = 0i64;
        let mut cache_write = 0i64;
        let mut cost: Option<f64> = None;

        let larger =
            |acc: Option<i64>, ledger: Option<i64>| acc.unwrap_or(0).max(ledger.unwrap_or(0));

        for row in rows {
            let (
                acc_in,
                acc_out,
                acc_total,
                acc_cr,
                acc_cw,
                acc_cost,
                l_in,
                l_out,
                l_total,
                l_cr,
                l_cw,
                l_cost,
            ) = row;
            input += larger(acc_in, l_in);
            output += larger(acc_out, l_out);
            total += larger(acc_total, l_total);
            cache_read += larger(acc_cr, l_cr);
            cache_write += larger(acc_cw, l_cw);
            if acc_cost.is_some() || l_cost.is_some() {
                let c = acc_cost.unwrap_or(0.0).max(l_cost.unwrap_or(0.0));
                cost = Some(cost.unwrap_or(0.0) + c);
            }
        }

        let opt = |v: i64| Some(i32::try_from(v).unwrap_or(i32::MAX));
        Ok(SessionUsageTotals {
            accumulated_usage: Usage::new(opt(input), opt(output), opt(total))
                .with_cache_tokens(opt(cache_read), opt(cache_write)),
            accumulated_cost: cost,
        })
    }

    async fn export_session(&self, id: &str) -> Result<String> {
        let session = self.get_session(id, true).await?;
        serde_json::to_string_pretty(&session).map_err(Into::into)
    }

    async fn import_session(
        &self,
        session_manager: &SessionManager,
        json: &str,
        session_type_override: Option<SessionType>,
    ) -> Result<Session> {
        let normalized =
            crate::session::import_formats::ImportFormat::convert_to_goose_session_json(json)?;
        let import: Session = serde_json::from_str(&normalized)?;

        let session = self
            .create_session(
                import.working_dir.clone(),
                import.name.clone(),
                session_type_override.unwrap_or(import.session_type),
                import.goose_mode,
            )
            .await?;

        let mut builder = session_manager
            .update(&session.id)
            .extension_data(import.extension_data)
            .usage(import.usage)
            .accumulated_usage(import.accumulated_usage)
            .accumulated_cost(import.accumulated_cost)
            .schedule_id(import.schedule_id)
            .recipe(import.recipe)
            .user_recipe_values(import.user_recipe_values);

        if import.user_set_name {
            builder = builder.user_provided_name(import.name.clone());
        }

        builder.apply().await?;

        if let Some(conversation) = import.conversation {
            self.replace_conversation(&session.id, &conversation)
                .await?;
        }

        self.get_session(&session.id, true).await
    }

    async fn copy_session(
        &self,
        session_manager: &SessionManager,
        session_id: &str,
        new_name: String,
    ) -> Result<Session> {
        let original_session = self.get_session(session_id, true).await?;

        let new_session = self
            .create_session(
                original_session.working_dir.clone(),
                new_name,
                original_session.session_type,
                original_session.goose_mode,
            )
            .await?;

        let mut builder = session_manager
            .update(&new_session.id)
            .extension_data(original_session.extension_data)
            .schedule_id(original_session.schedule_id)
            .recipe(original_session.recipe)
            .user_recipe_values(original_session.user_recipe_values);

        if let Some(project_id) = original_session.project_id {
            builder = builder.project_id(Some(project_id));
        }
        if let Some(provider_name) = original_session.provider_name {
            builder = builder.provider_name(provider_name);
        }
        if let Some(model_config) = original_session.model_config {
            builder = builder.model_config(model_config);
        }
        builder = builder.goose_mode(original_session.goose_mode);

        builder.apply().await?;

        if let Some(conversation) = original_session.conversation {
            self.replace_conversation(&new_session.id, &conversation)
                .await?;
        }

        self.get_session(&new_session.id, true).await
    }

    async fn truncate_conversation(&self, session_id: &str, timestamp: i64) -> Result<()> {
        let pool = self.pool().await?;
        sqlx::query("DELETE FROM messages WHERE session_id = ? AND created_timestamp >= ?")
            .bind(session_id)
            .bind(timestamp)
            .execute(pool)
            .await?;

        Ok(())
    }

    async fn truncate_conversation_from_message(
        &self,
        session_id: &str,
        message_id: &str,
    ) -> Result<()> {
        let pool = self.pool().await?;
        let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;

        let boundary = sqlx::query_as::<_, (i64, i64)>(
            "SELECT id, created_timestamp FROM messages WHERE session_id = ? AND message_id = ? ORDER BY created_timestamp, id LIMIT 1",
        )
        .bind(session_id)
        .bind(message_id)
        .fetch_optional(&mut *tx)
        .await?;

        if let Some((boundary_id, boundary_timestamp)) = boundary {
            sqlx::query(
                "DELETE FROM messages WHERE session_id = ? AND (created_timestamp > ? OR (created_timestamp = ? AND id >= ?))",
            )
            .bind(session_id)
            .bind(boundary_timestamp)
            .bind(boundary_timestamp)
            .bind(boundary_id)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(())
    }

    async fn search_chat_history(
        &self,
        query: &str,
        limit: Option<usize>,
        after_date: Option<chrono::DateTime<chrono::Utc>>,
        before_date: Option<chrono::DateTime<chrono::Utc>>,
        exclude_session_id: Option<String>,
        session_types: Vec<SessionType>,
    ) -> Result<crate::session::chat_history_search::ChatRecallResults> {
        use crate::session::chat_history_search::ChatHistorySearch;
        let pool = self.pool().await?;
        ChatHistorySearch::new(
            pool,
            query,
            limit,
            after_date,
            before_date,
            exclude_session_id,
            session_types,
        )
        .execute()
        .await
    }

    async fn update_message_metadata<F>(
        &self,
        session_id: &str,
        message_id: &str,
        f: F,
    ) -> Result<()>
    where
        F: FnOnce(
            bcaip_provider_types::conversations::MessageMetadata,
        ) -> bcaip_provider_types::conversations::MessageMetadata,
    {
        let pool = self.pool().await?;
        let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;

        let current_metadata_json = sqlx::query_scalar::<_, String>(
            "SELECT metadata_json FROM messages WHERE message_id = ? AND session_id = ?",
        )
        .bind(message_id)
        .bind(session_id)
        .fetch_one(&mut *tx)
        .await?;

        let current_metadata: bcaip_provider_types::conversations::MessageMetadata =
            serde_json::from_str(&current_metadata_json)?;

        let new_metadata = f(current_metadata);
        let metadata_json = serde_json::to_string(&new_metadata)?;

        sqlx::query(
            "UPDATE messages SET metadata_json = ? WHERE message_id = ? AND session_id = ?",
        )
        .bind(metadata_json)
        .bind(message_id)
        .bind(session_id)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;

        Ok(())
    }

    async fn update_tool_request_meta(
        &self,
        session_id: &str,
        tool_call_id: &str,
        patch: serde_json::Value,
    ) -> Result<()> {
        let pool = self.pool().await?;
        let rows = sqlx::query_as::<_, (Option<String>, String)>(
            "SELECT message_id, content_json FROM messages \
             WHERE session_id = ? \
             ORDER BY id DESC \
             LIMIT 100",
        )
        .bind(session_id)
        .fetch_all(pool)
        .await?;

        for (message_id, content_json) in rows {
            let content: Vec<MessageContent> = serde_json::from_str(&content_json)?;
            let contains_tool_request = content.iter().any(|block| {
                matches!(
                    block,
                    MessageContent::ToolRequest(tool_request)
                        if tool_request.id == tool_call_id
                )
            });
            if contains_tool_request {
                let Some(message_id) = message_id else {
                    return Ok(());
                };
                return self
                    .update_tool_request_meta_by_message_id(
                        session_id,
                        &message_id,
                        tool_call_id,
                        patch,
                    )
                    .await;
            }
        }

        Ok(())
    }

    /// Patch `tool_meta` on a specific `ToolRequest` within a stored message's
    /// `content_json`. Finds the row(s) with matching `message_id`, scans each
    /// row's content for a `ToolRequest` with the given `tool_call_id`, and
    /// merges `patch` into its `tool_meta`. Uses `BEGIN IMMEDIATE` so
    /// concurrent writers serialize correctly.
    async fn update_tool_request_meta_by_message_id(
        &self,
        session_id: &str,
        message_id: &str,
        tool_call_id: &str,
        patch: serde_json::Value,
    ) -> Result<()> {
        let pool = self.pool().await?;
        let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;

        let rows = sqlx::query_as::<_, (i64, String)>(
            "SELECT id, content_json FROM messages \
             WHERE session_id = ? AND message_id = ? \
             ORDER BY id ASC",
        )
        .bind(session_id)
        .bind(message_id)
        .fetch_all(&mut *tx)
        .await?;

        for (row_id, content_json) in rows {
            let mut content: Vec<MessageContent> = serde_json::from_str(&content_json)?;
            let mut found = false;
            for block in &mut content {
                if let MessageContent::ToolRequest(tr) = block {
                    if tr.id == tool_call_id {
                        tr.tool_meta = Some(merge_tool_meta(tr.tool_meta.take(), &patch));
                        found = true;
                        break;
                    }
                }
            }
            if !found {
                continue;
            }

            let updated_json = serde_json::to_string(&content)?;
            sqlx::query("UPDATE messages SET content_json = ? WHERE id = ?")
                .bind(updated_json)
                .bind(row_id)
                .execute(&mut *tx)
                .await?;
            tx.commit().await?;
            return Ok(());
        }

        tx.commit().await?;
        Ok(())
    }
}

/// Merge a JSON object `patch` into an existing optional object value,
/// preserving keys not present in the patch.
fn merge_tool_meta(
    existing: Option<serde_json::Value>,
    patch: &serde_json::Value,
) -> serde_json::Value {
    let mut base = match existing {
        Some(serde_json::Value::Object(map)) => map,
        _ => serde_json::Map::new(),
    };
    if let serde_json::Value::Object(patch_map) = patch {
        for (k, v) in patch_map {
            base.insert(k.clone(), v.clone());
        }
    }
    serde_json::Value::Object(base)
}
