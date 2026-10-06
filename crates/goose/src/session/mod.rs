mod chat_history_search;
mod diagnostics;
mod export_markdown;
mod extension_data;
mod import_formats;
mod last_message_snippet;
mod legacy;
mod session_manager;
mod session_naming;

pub use diagnostics::{
    DiagnosticsConfig, DiagnosticsError, DiagnosticsExtensions, DiagnosticsLevel, DiagnosticsLogs,
    DiagnosticsPrompt, DiagnosticsReport, DiagnosticsScheduledRecipe, DiagnosticsTextFile,
    SystemInfo, config_path, generate_diagnostics, get_system_info, latest_llm_log_path,
    read_capped, read_tail, recent_cli_log_paths,
};
pub use export_markdown::{
    export_session_to_markdown, message_to_markdown, user_projected_message_to_markdown,
};
pub use extension_data::{EnabledExtensionsState, ExtensionData, ExtensionState, TodoState};
pub use import_formats::{ImportFormat, detect_format};
pub use legacy::{list_sessions, load_session};
pub use session_manager::{
    CURRENT_SCHEMA_VERSION, DB_NAME, SESSIONS_FOLDER, Session, SessionInsights, SessionManager,
    SessionNameUpdate, SessionStorage, SessionType, SessionUpdateBuilder, SessionUsageTotals,
};
pub(crate) use session_manager::{SessionListCursor, SessionListFilters, SessionListPageQuery};
