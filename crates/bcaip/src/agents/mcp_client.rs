use crate::action_required_manager::{ActionRequiredManager, ElicitationOutcome};
use crate::agents::{extension_manager::ExtensionManager, tool_execution::ToolCallContext};
use crate::session_context::{SESSION_ID_HEADER, TOOL_CALL_REQUEST_ID_HEADER, WORKING_DIR_HEADER};
#[expect(deprecated)]
use rmcp::model::{
    ElicitRequestParams, ElicitResult, ListRootsResult, LoggingMessageNotification, Root,
};
use rmcp::model::{
    ElicitationAction, ErrorCode, ExtensionCapabilities, Extensions, JsonObject, MetaObject,
};
use rmcp::{
    ClientHandler, ErrorData, Peer, RoleClient, ServiceError,
    model::{
        CallToolRequestParams, CallToolResult, CancelledNotificationParam, ClientCapabilities,
        ClientConfig, ClientRequest, GetPromptRequestParams, GetPromptResult, Implementation,
        InitializeRequestParams, InitializeResult, ListPromptsResult, ListResourcesResult,
        ListToolsResult, Notification, PaginatedRequestParams, ProtocolVersion,
        ReadResourceRequestParams, ReadResourceResult, Request, RequestId, RequestOptionalParam,
        ServerNotification, ServerResult,
    },
    service::{
        ClientInitializeError, ClientLifecycleMode, ClientServiceExt, PeerRequestOptions,
        RequestContext, RequestHandle, RunningService, ServiceRole,
    },
    transport::IntoTransport,
};
use serde_json::Value;
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex as StdMutex, Weak},
    time::Duration,
};
use tokio::sync::{
    Mutex,
    mpsc::{self, Sender},
};
use tokio_util::sync::CancellationToken;
pub type BoxError = Box<dyn std::error::Error + Sync + Send>;

pub type Error = rmcp::ServiceError;

const MCP_APPS_UI_EXTENSION_ID: &str = "io.modelcontextprotocol/ui";
const MCP_APPS_UI_MIME_TYPE: &str = "text/html;profile=mcp-app";

fn default_mcp_apps_ui_extensions() -> ExtensionCapabilities {
    let mut extensions = ExtensionCapabilities::new();
    let mut ui_extension_settings = JsonObject::new();
    ui_extension_settings.insert(
        "mimeTypes".to_string(),
        serde_json::json!([MCP_APPS_UI_MIME_TYPE]),
    );
    extensions.insert(MCP_APPS_UI_EXTENSION_ID.to_string(), ui_extension_settings);
    extensions
}

#[derive(Debug, Clone, Default)]
pub struct BcaipMcpHostInfo {
    pub explicit_extensions: bool,
    pub extensions: ExtensionCapabilities,
    pub client_name: Option<String>,
    pub client_version: Option<String>,
}

impl BcaipMcpHostInfo {
    pub fn mcpui_enabled(&self) -> bool {
        self.extensions.contains_key(MCP_APPS_UI_EXTENSION_ID)
    }
}

#[async_trait::async_trait]
pub trait McpClientTrait: Send + Sync {
    async fn list_tools(
        &self,
        session_id: &str,
        next_cursor: Option<String>,
        cancel_token: CancellationToken,
    ) -> Result<ListToolsResult, Error>;

    async fn call_tool(
        &self,
        ctx: &ToolCallContext,
        name: &str,
        arguments: Option<JsonObject>,
        cancel_token: CancellationToken,
    ) -> Result<CallToolResult, Error>;

    fn get_info(&self) -> Option<&InitializeResult>;

    /// Return the extension's current instructions. The default reads from
    /// `get_info()`, but platform extensions can override this to provide
    /// dynamically computed instructions (e.g. freshly discovered skills).
    fn get_instructions(&self) -> Option<String> {
        self.get_info().and_then(|info| info.instructions.clone())
    }

    async fn list_resources(
        &self,
        _session_id: &str,
        _next_cursor: Option<String>,
        _cancel_token: CancellationToken,
    ) -> Result<ListResourcesResult, Error> {
        Err(Error::TransportClosed)
    }

    async fn read_resource(
        &self,
        _session_id: &str,
        _uri: &str,
        _cancel_token: CancellationToken,
    ) -> Result<ReadResourceResult, Error> {
        Err(Error::TransportClosed)
    }

    async fn list_prompts(
        &self,
        _session_id: &str,
        _next_cursor: Option<String>,
        _cancel_token: CancellationToken,
    ) -> Result<ListPromptsResult, Error> {
        Err(Error::TransportClosed)
    }

    async fn get_prompt(
        &self,
        _session_id: &str,
        _name: &str,
        _arguments: Value,
        _cancel_token: CancellationToken,
    ) -> Result<GetPromptResult, Error> {
        Err(Error::TransportClosed)
    }

    async fn subscribe(&self) -> mpsc::Receiver<ServerNotification> {
        mpsc::channel(1).1
    }

    async fn get_moim(&self, _session_id: &str) -> Option<String> {
        None
    }

    async fn update_working_dir(&self, _new_dir: PathBuf) -> Result<(), Error> {
        Ok(())
    }
}

struct ActiveToolCallGuard {
    active_tool_calls: Arc<StdMutex<HashMap<String, Vec<String>>>>,
    session_id: String,
    tool_call_request_id: String,
}

impl Drop for ActiveToolCallGuard {
    fn drop(&mut self) {
        let mut active_tool_calls = self
            .active_tool_calls
            .lock()
            .expect("active_tool_calls mutex poisoned");
        if let Some(calls) = active_tool_calls.get_mut(&self.session_id) {
            if let Some(pos) = calls.iter().position(|id| id == &self.tool_call_request_id) {
                calls.remove(pos);
            }
            if calls.is_empty() {
                active_tool_calls.remove(&self.session_id);
            }
        }
    }
}

pub struct BcaipClient {
    notification_handlers: Arc<Mutex<Vec<Sender<ServerNotification>>>>,
    session_id: Mutex<Option<String>>,
    active_tool_calls: Arc<StdMutex<HashMap<String, Vec<String>>>>,
    client_name: String,
    capabilities: BcaipMcpClientCapabilities,
    working_dir: Arc<tokio::sync::RwLock<PathBuf>>,
    action_required: Arc<ActionRequiredManager>,
    extension_manager: Weak<ExtensionManager>,
}

impl BcaipClient {
    pub(crate) fn new(
        handlers: Arc<Mutex<Vec<Sender<ServerNotification>>>>,
        client_name: String,
        capabilities: BcaipMcpClientCapabilities,
        working_dir: PathBuf,
        action_required: Arc<ActionRequiredManager>,
        extension_manager: Weak<ExtensionManager>,
    ) -> Self {
        BcaipClient {
            notification_handlers: handlers,
            session_id: Mutex::new(None),
            active_tool_calls: Arc::new(StdMutex::new(HashMap::new())),
            client_name,
            capabilities,
            working_dir: Arc::new(tokio::sync::RwLock::new(working_dir)),
            action_required,
            extension_manager,
        }
    }

    pub fn shared_working_dir(&self) -> Arc<tokio::sync::RwLock<PathBuf>> {
        self.working_dir.clone()
    }

    async fn set_session_id(&self, session_id: &str) {
        let mut slot = self.session_id.lock().await;
        assert!(
            slot.as_deref().is_none_or(|s| s == session_id),
            "McpClient received requests from different sessions"
        );
        *slot = Some(session_id.to_string());
    }

    async fn handle_tool_list_changed(&self) {
        if let Some(extension_manager) = self.extension_manager.upgrade() {
            extension_manager
                .invalidate_tools_cache_and_bump_version()
                .await;
        }
    }

    async fn current_session_id(&self) -> Option<String> {
        self.session_id.lock().await.clone()
    }

    async fn resolve_session_id(&self, extensions: &Extensions) -> Option<String> {
        // Prefer explicit MCP metadata, then the active request scope.
        let current_session_id = self.current_session_id().await;
        Self::session_id_from_extensions(extensions).or(current_session_id)
    }

    fn session_id_from_extensions(extensions: &Extensions) -> Option<String> {
        let meta = extensions.get::<MetaObject>()?;
        meta.0
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(SESSION_ID_HEADER))
            .and_then(|(_, value)| value.as_str())
            .map(|value| value.to_string())
    }

    fn tool_call_request_id_from_extensions(extensions: &Extensions) -> Option<String> {
        let meta = extensions.get::<MetaObject>()?;
        meta.0
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(TOOL_CALL_REQUEST_ID_HEADER))
            .and_then(|(_, value)| value.as_str())
            .map(|value| value.to_string())
    }

    fn register_active_tool_call(
        &self,
        session_id: &str,
        tool_call_request_id: &str,
    ) -> ActiveToolCallGuard {
        self.active_tool_calls
            .lock()
            .expect("active_tool_calls mutex poisoned")
            .entry(session_id.to_string())
            .or_default()
            .push(tool_call_request_id.to_string());
        ActiveToolCallGuard {
            active_tool_calls: self.active_tool_calls.clone(),
            session_id: session_id.to_string(),
            tool_call_request_id: tool_call_request_id.to_string(),
        }
    }

    fn resolve_tool_call_request_id(
        &self,
        session_id: &str,
        extensions: &Extensions,
    ) -> Result<String, ErrorData> {
        if let Some(tool_call_request_id) = Self::tool_call_request_id_from_extensions(extensions) {
            return Ok(tool_call_request_id);
        }

        let active_tool_calls = self
            .active_tool_calls
            .lock()
            .expect("active_tool_calls mutex poisoned");
        match active_tool_calls.get(session_id).map(Vec::as_slice) {
            Some([tool_call_request_id]) => Ok(tool_call_request_id.clone()),
            Some(calls) if calls.len() > 1 => Err(ErrorData::new(
                ErrorCode::INTERNAL_ERROR,
                "Cannot correlate elicitation request: multiple tool calls are active and the \
                 server did not echo the tool call request id",
                None,
            )),
            _ => Err(ErrorData::new(
                ErrorCode::INTERNAL_ERROR,
                "Could not resolve tool call request id for elicitation request",
                None,
            )),
        }
    }

    fn resolved_extensions(&self) -> ExtensionCapabilities {
        if let Some(host_info) = &self.capabilities.host_info
            && host_info.explicit_extensions
        {
            return host_info.extensions.clone();
        }

        if self.capabilities.mcpui {
            return default_mcp_apps_ui_extensions();
        }

        ExtensionCapabilities::new()
    }

    fn resolved_client_info(&self) -> Implementation {
        let name = self
            .capabilities
            .host_info
            .as_ref()
            .and_then(|host_info| host_info.client_name.clone())
            .unwrap_or_else(|| self.client_name.clone());
        let version = self
            .capabilities
            .host_info
            .as_ref()
            .and_then(|host_info| host_info.client_version.clone())
            .unwrap_or_else(|| {
                std::env::var("BCAIP_MCP_CLIENT_VERSION")
                    .unwrap_or(env!("CARGO_PKG_VERSION").to_owned())
            });

        Implementation::new(name, version)
    }
}

#[expect(deprecated)]
fn working_dir_roots(dir: &std::path::Path) -> ListRootsResult {
    let uri = url::Url::from_file_path(dir)
        .map(|u| u.to_string())
        .unwrap_or_else(|()| format!("file://{}", dir.display()));
    ListRootsResult::new(vec![Root::new(uri).with_name("working_directory")])
}

/// Fan out a notification to all subscribers, dropping senders whose receivers are gone.
fn fan_out_notification(
    handlers: &mut Vec<Sender<ServerNotification>>,
    notification: ServerNotification,
) {
    handlers.retain(|handler| match handler.try_send(notification.clone()) {
        Ok(()) => true,
        Err(mpsc::error::TrySendError::Full(_)) => true,
        Err(mpsc::error::TrySendError::Closed(_)) => false,
    });
}

impl ClientHandler for BcaipClient {
    #[expect(deprecated)]
    async fn list_roots(
        &self,
        _context: RequestContext<RoleClient>,
    ) -> Result<ListRootsResult, ErrorData> {
        Ok(working_dir_roots(&self.working_dir.read().await))
    }

    async fn on_progress(
        &self,
        params: rmcp::model::ProgressNotificationParam,
        context: rmcp::service::NotificationContext<rmcp::RoleClient>,
    ) {
        let mut not = Notification::new(params);
        not.extensions = context.extensions;
        fan_out_notification(
            &mut *self.notification_handlers.lock().await,
            ServerNotification::ProgressNotification(not),
        );
    }

    async fn on_tool_list_changed(&self, _context: rmcp::service::NotificationContext<RoleClient>) {
        self.handle_tool_list_changed().await;
    }

    #[expect(deprecated)]
    async fn on_logging_message(
        &self,
        params: rmcp::model::LoggingMessageNotificationParam,
        context: rmcp::service::NotificationContext<rmcp::RoleClient>,
    ) {
        let mut notification = LoggingMessageNotification::new(params);
        notification.extensions = context.extensions;
        fan_out_notification(
            &mut *self.notification_handlers.lock().await,
            ServerNotification::LoggingMessageNotification(notification),
        );
    }

    async fn create_elicitation(
        &self,
        request: ElicitRequestParams,
        context: RequestContext<RoleClient>,
    ) -> Result<ElicitResult, ErrorData> {
        if let Some(handler) = &self.capabilities.elicitation_handler {
            return Ok(handler(&request));
        }

        let session_id = self
            .resolve_session_id(&context.extensions)
            .await
            .ok_or_else(|| {
                ErrorData::new(
                    ErrorCode::INTERNAL_ERROR,
                    "Could not resolve session id for elicitation request",
                    None,
                )
            })?;
        let tool_call_request_id =
            self.resolve_tool_call_request_id(&session_id, &context.extensions)?;

        let (message, schema_value) = match &request {
            ElicitRequestParams::FormElicitationParams {
                message,
                requested_schema,
                ..
            } => {
                let schema_value = serde_json::to_value(requested_schema).map_err(|e| {
                    ErrorData::new(
                        ErrorCode::INTERNAL_ERROR,
                        format!("Failed to serialize elicitation schema: {}", e),
                        None,
                    )
                })?;
                (message.clone(), schema_value)
            }
            ElicitRequestParams::UrlElicitationParams { message, url, .. } => {
                (message.clone(), serde_json::json!({ "url": url }))
            }
            _ => (String::new(), serde_json::json!({})),
        };

        self.action_required
            .request_and_wait(
                session_id,
                tool_call_request_id,
                message,
                schema_value,
                Duration::from_secs(300),
            )
            .await
            .map(|response| match response {
                ElicitationOutcome::Accept(user_data) => {
                    ElicitResult::new(ElicitationAction::Accept).with_content(user_data)
                }
                ElicitationOutcome::Decline => ElicitResult::new(ElicitationAction::Decline),
                ElicitationOutcome::Cancel => ElicitResult::new(ElicitationAction::Cancel),
            })
            .map_err(|e| {
                ErrorData::new(
                    ErrorCode::INTERNAL_ERROR,
                    format!("Elicitation request timed out or failed: {}", e),
                    None,
                )
            })
    }

    fn get_info(&self) -> ClientConfig {
        let extensions = self.resolved_extensions();

        InitializeRequestParams::new(
            #[expect(deprecated)]
            ClientCapabilities::builder()
                .enable_roots()
                .enable_extensions_with(extensions)
                .enable_elicitation()
                .build(),
            self.resolved_client_info(),
        )
        .with_protocol_version(
            self.capabilities
                .protocol_version
                .clone()
                .unwrap_or_default(),
        )
    }
}

pub type ElicitationHandler = Arc<dyn Fn(&ElicitRequestParams) -> ElicitResult + Send + Sync>;

#[derive(Clone, Default)]
pub struct BcaipMcpClientCapabilities {
    pub mcpui: bool,
    pub host_info: Option<BcaipMcpHostInfo>,
    pub elicitation_handler: Option<ElicitationHandler>,
    pub protocol_version: Option<ProtocolVersion>,
}

impl std::fmt::Debug for BcaipMcpClientCapabilities {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("BcaipMcpClientCapabilities")
            .field("mcpui", &self.mcpui)
            .field("host_info", &self.host_info)
            .field("elicitation_handler", &self.elicitation_handler.is_some())
            .field("protocol_version", &self.protocol_version)
            .finish()
    }
}

#[derive(Clone)]
pub(crate) struct ConnectContext {
    pub timeout: Duration,
    pub client_name: String,
    pub capabilities: BcaipMcpClientCapabilities,
    pub working_dir: PathBuf,
    pub docker_container: Option<String>,
    pub action_required: Arc<ActionRequiredManager>,
    pub extension_manager: Weak<ExtensionManager>,
}

/// The MCP client is the interface for MCP operations.
pub struct McpClient {
    client: Mutex<Arc<RunningService<RoleClient, BcaipClient>>>,
    notification_subscribers: Arc<Mutex<Vec<mpsc::Sender<ServerNotification>>>>,
    server_info: Option<InitializeResult>,
    timeout: std::time::Duration,
    docker_container: Option<String>,
}

impl McpClient {
    pub(crate) async fn connect<T, E, A>(
        transport: T,
        ctx: ConnectContext,
    ) -> Result<Self, ClientInitializeError>
    where
        T: IntoTransport<RoleClient, E, A>,
        E: std::error::Error + From<std::io::Error> + Send + Sync + 'static,
    {
        let ConnectContext {
            timeout,
            client_name,
            capabilities,
            working_dir,
            docker_container,
            action_required,
            extension_manager,
        } = ctx;
        let notification_subscribers =
            Arc::new(Mutex::new(Vec::<mpsc::Sender<ServerNotification>>::new()));

        let client = BcaipClient::new(
            notification_subscribers.clone(),
            client_name.clone(),
            capabilities.clone(),
            working_dir,
            action_required,
            extension_manager,
        );
        let client: rmcp::service::RunningService<rmcp::RoleClient, BcaipClient> =
            if let Some(protocol_version) = capabilities.protocol_version {
                let lifecycle = if protocol_version >= ProtocolVersion::STANDARD_HEADERS {
                    ClientLifecycleMode::Discover {
                        preferred_versions: vec![protocol_version],
                    }
                } else {
                    ClientLifecycleMode::Initialize
                };
                client.serve_with_lifecycle(transport, lifecycle).await?
            } else {
                client
                    .serve_with_lifecycle(
                        transport,
                        ClientLifecycleMode::Auto {
                            preferred_versions: vec![
                                ProtocolVersion::V_2026_07_28,
                                ProtocolVersion::V_2025_11_25,
                            ],
                            legacy_version: Some(ProtocolVersion::V_2025_11_25),
                        },
                    )
                    .await?
            };
        let server_info = client.peer_info().map(|info| {
            let mut initialize_result = InitializeResult::new(info.capabilities.clone())
                .with_protocol_version(info.protocol_version.clone());
            if let Some(server_info) = &info.server_info {
                initialize_result = initialize_result.with_server_info(server_info.clone());
            }
            initialize_result.instructions = info.instructions.clone();
            initialize_result.meta = info.meta.clone();
            initialize_result
        });

        Ok(Self {
            client: Mutex::new(Arc::new(client)),
            notification_subscribers,
            server_info,
            timeout,
            docker_container,
        })
    }

    pub fn docker_container(&self) -> Option<&str> {
        self.docker_container.as_deref()
    }

    async fn do_update_working_dir(&self, new_dir: PathBuf) -> Result<(), Error> {
        let client = self.client.lock().await;
        let shared = client.service().shared_working_dir();
        *shared.write().await = new_dir;
        client.peer().notify_roots_list_changed().await?;
        Ok(())
    }

    async fn send_request_with_context(
        &self,
        session_id: &str,
        working_dir: Option<&str>,
        tool_call_request_id: Option<&str>,
        request: ClientRequest,
        cancel_token: CancellationToken,
    ) -> Result<ServerResult, Error> {
        let request = inject_session_context_into_request(
            request,
            Some(session_id),
            working_dir,
            tool_call_request_id,
        );
        let active_tool_call = tool_call_request_id.filter(|id| !id.is_empty());
        // The inner mutex is held only for the send; the actual response wait
        // happens outside the lock so concurrent calls can overlap. The guard
        // unregisters the active tool call on drop, covering cancellation and
        // dropped reply streams as well as normal completion.
        let (handle, _active_tool_call_guard) = {
            let client = self.client.lock().await;
            client.service().set_session_id(session_id).await;
            let guard = active_tool_call.map(|tool_call_request_id| {
                client
                    .service()
                    .register_active_tool_call(session_id, tool_call_request_id)
            });
            let handle = client
                .send_cancellable_request(request, PeerRequestOptions::no_options())
                .await?;
            (handle, guard)
        };

        await_response(handle, self.timeout, &cancel_token).await
    }
}

async fn await_response(
    handle: RequestHandle<RoleClient>,
    timeout: Duration,
    cancel_token: &CancellationToken,
) -> Result<<RoleClient as ServiceRole>::PeerResp, ServiceError> {
    let receiver = handle.rx;
    let peer = handle.peer;
    let request_id = handle.id;
    tokio::select! {
        result = receiver => {
            result.map_err(|_e| ServiceError::TransportClosed)?
        }
        _ = tokio::time::sleep(timeout) => {
            send_cancel_message(&peer, request_id, Some("timed out".to_owned())).await?;
            Err(ServiceError::Timeout{timeout})
        }
        _ = cancel_token.cancelled() => {
            send_cancel_message(&peer, request_id, Some("operation cancelled".to_owned())).await?;
            Err(ServiceError::Cancelled { reason: None })
        }
    }
}

async fn send_cancel_message(
    peer: &Peer<RoleClient>,
    request_id: RequestId,
    reason: Option<String>,
) -> Result<(), ServiceError> {
    peer.send_notification(
        Notification::new(CancelledNotificationParam::new(Some(request_id), reason)).into(),
    )
    .await
}

#[async_trait::async_trait]
impl McpClientTrait for McpClient {
    fn get_info(&self) -> Option<&InitializeResult> {
        self.server_info.as_ref()
    }

    async fn list_resources(
        &self,
        session_id: &str,
        cursor: Option<String>,
        cancel_token: CancellationToken,
    ) -> Result<ListResourcesResult, Error> {
        let res = self
            .send_request_with_context(
                session_id,
                None,
                None,
                ClientRequest::ListResourcesRequest(RequestOptionalParam::with_param(
                    PaginatedRequestParams::default().with_cursor(cursor),
                )),
                cancel_token,
            )
            .await?;

        match res {
            ServerResult::ListResourcesResult(result) => Ok(result),
            _ => Err(ServiceError::UnexpectedResponse),
        }
    }

    async fn read_resource(
        &self,
        session_id: &str,
        uri: &str,
        cancel_token: CancellationToken,
    ) -> Result<ReadResourceResult, Error> {
        let params = ReadResourceRequestParams::new(uri.to_string());
        let client = self.client.lock().await.clone();
        if client
            .peer_info()
            .is_some_and(|info| info.protocol_version == ProtocolVersion::V_2026_07_28)
        {
            client.service().set_session_id(session_id).await;
            return tokio::select! {
                result = client.read_resource(params) => result,
                _ = tokio::time::sleep(self.timeout) => Err(ServiceError::Timeout { timeout: self.timeout }),
                _ = cancel_token.cancelled() => Err(ServiceError::Cancelled { reason: None }),
            };
        }
        drop(client);

        let res = self
            .send_request_with_context(
                session_id,
                None,
                None,
                ClientRequest::ReadResourceRequest(Request::new(params)),
                cancel_token,
            )
            .await?;

        match res {
            ServerResult::ReadResourceResult(result) => Ok(result),
            _ => Err(ServiceError::UnexpectedResponse),
        }
    }

    async fn list_tools(
        &self,
        session_id: &str,
        cursor: Option<String>,
        cancel_token: CancellationToken,
    ) -> Result<ListToolsResult, Error> {
        let res = self
            .send_request_with_context(
                session_id,
                None,
                None,
                ClientRequest::ListToolsRequest(RequestOptionalParam::with_param(
                    PaginatedRequestParams::default().with_cursor(cursor),
                )),
                cancel_token,
            )
            .await?;

        match res {
            ServerResult::ListToolsResult(result) => Ok(result),
            _ => Err(ServiceError::UnexpectedResponse),
        }
    }

    async fn call_tool(
        &self,
        ctx: &ToolCallContext,
        name: &str,
        arguments: Option<JsonObject>,
        cancel_token: CancellationToken,
    ) -> Result<CallToolResult, Error> {
        let mut params = CallToolRequestParams::new(name.to_string());
        if let Some(args) = arguments {
            params = params.with_arguments(args);
        }
        let protocol_version = {
            let client = self.client.lock().await;
            client.peer_info().map(|info| info.protocol_version.clone())
        };
        if protocol_version.as_ref() == Some(&ProtocolVersion::V_2026_07_28) {
            let extensions = inject_session_context_into_extensions(
                Extensions::new(),
                Some(&ctx.session_id),
                ctx.working_dir_str(),
                ctx.tool_call_request_id.as_deref(),
            );
            if let Some(meta) = extensions.get::<MetaObject>() {
                params.meta.get_or_insert_default().0.0.extend(
                    meta.0
                        .iter()
                        .map(|(key, value)| (key.clone(), value.clone())),
                );
            }
            let client = self.client.lock().await.clone();
            client.service().set_session_id(&ctx.session_id).await;
            let _active_tool_call_guard = ctx
                .tool_call_request_id
                .as_deref()
                .filter(|id| !id.is_empty())
                .map(|tool_call_request_id| {
                    client
                        .service()
                        .register_active_tool_call(&ctx.session_id, tool_call_request_id)
                });
            return tokio::select! {
                result = client.call_tool(params) => result,
                _ = tokio::time::sleep(self.timeout) => {
                    Err(ServiceError::Timeout { timeout: self.timeout })
                }
                _ = cancel_token.cancelled() => {
                    Err(ServiceError::Cancelled { reason: None })
                }
            };
        }

        let request = ClientRequest::CallToolRequest(Request::new(params));

        let result = self
            .send_request_with_context(
                &ctx.session_id,
                ctx.working_dir_str(),
                ctx.tool_call_request_id.as_deref(),
                request,
                cancel_token,
            )
            .await;

        match result? {
            ServerResult::CallToolResult(result) => Ok(result),
            _ => Err(ServiceError::UnexpectedResponse),
        }
    }

    async fn list_prompts(
        &self,
        session_id: &str,
        cursor: Option<String>,
        cancel_token: CancellationToken,
    ) -> Result<ListPromptsResult, Error> {
        let res = self
            .send_request_with_context(
                session_id,
                None,
                None,
                ClientRequest::ListPromptsRequest(RequestOptionalParam::with_param(
                    PaginatedRequestParams::default().with_cursor(cursor),
                )),
                cancel_token,
            )
            .await?;

        match res {
            ServerResult::ListPromptsResult(result) => Ok(result),
            _ => Err(ServiceError::UnexpectedResponse),
        }
    }

    async fn get_prompt(
        &self,
        session_id: &str,
        name: &str,
        arguments: Value,
        cancel_token: CancellationToken,
    ) -> Result<GetPromptResult, Error> {
        let arguments = match arguments {
            Value::Object(map) => Some(map),
            _ => None,
        };
        let mut params = GetPromptRequestParams::new(name.to_string());
        if let Some(args) = arguments {
            params = params.with_arguments(args);
        }
        let client = self.client.lock().await.clone();
        if client
            .peer_info()
            .is_some_and(|info| info.protocol_version == ProtocolVersion::V_2026_07_28)
        {
            client.service().set_session_id(session_id).await;
            return tokio::select! {
                result = client.get_prompt(params) => result,
                _ = tokio::time::sleep(self.timeout) => Err(ServiceError::Timeout { timeout: self.timeout }),
                _ = cancel_token.cancelled() => Err(ServiceError::Cancelled { reason: None }),
            };
        }
        drop(client);

        let res = self
            .send_request_with_context(
                session_id,
                None,
                None,
                ClientRequest::GetPromptRequest(Request::new(params)),
                cancel_token,
            )
            .await?;

        match res {
            ServerResult::GetPromptResult(result) => Ok(result),
            _ => Err(ServiceError::UnexpectedResponse),
        }
    }

    async fn subscribe(&self) -> mpsc::Receiver<ServerNotification> {
        let (tx, rx) = mpsc::channel(16);
        self.notification_subscribers.lock().await.push(tx);
        rx
    }

    async fn update_working_dir(&self, new_dir: PathBuf) -> Result<(), Error> {
        self.do_update_working_dir(new_dir).await
    }
}

/// Injects the given session_id and working_dir into Extensions._meta.
/// None (or empty) removes any existing values.
fn inject_session_context_into_extensions(
    mut extensions: Extensions,
    session_id: Option<&str>,
    working_dir: Option<&str>,
    tool_call_request_id: Option<&str>,
) -> Extensions {
    let session_id = session_id.filter(|id| !id.is_empty());
    let working_dir = working_dir.filter(|dir| !dir.is_empty());
    let tool_call_request_id = tool_call_request_id.filter(|id| !id.is_empty());
    let mut meta_map = extensions
        .get::<MetaObject>()
        .map(|meta| meta.0.clone())
        .unwrap_or_default();

    // JsonObject is case-sensitive, so we use retain for case-insensitive removal
    meta_map.retain(|k, _| {
        !k.eq_ignore_ascii_case(SESSION_ID_HEADER)
            && !k.eq_ignore_ascii_case(WORKING_DIR_HEADER)
            && !k.eq_ignore_ascii_case(TOOL_CALL_REQUEST_ID_HEADER)
    });

    if let Some(session_id) = session_id {
        meta_map.insert(
            SESSION_ID_HEADER.to_string(),
            Value::String(session_id.to_string()),
        );
    }

    if let Some(working_dir) = working_dir {
        meta_map.insert(
            WORKING_DIR_HEADER.to_string(),
            Value::String(working_dir.to_string()),
        );
    }

    if let Some(tool_call_request_id) = tool_call_request_id {
        meta_map.insert(
            TOOL_CALL_REQUEST_ID_HEADER.to_string(),
            Value::String(tool_call_request_id.to_string()),
        );
    }

    extensions.insert(MetaObject(meta_map));
    extensions
}

fn inject_session_context_into_request(
    request: ClientRequest,
    session_id: Option<&str>,
    working_dir: Option<&str>,
    tool_call_request_id: Option<&str>,
) -> ClientRequest {
    match request {
        ClientRequest::ListResourcesRequest(mut req) => {
            req.extensions = inject_session_context_into_extensions(
                req.extensions,
                session_id,
                working_dir,
                None,
            );
            ClientRequest::ListResourcesRequest(req)
        }
        ClientRequest::ReadResourceRequest(mut req) => {
            req.extensions = inject_session_context_into_extensions(
                req.extensions,
                session_id,
                working_dir,
                None,
            );
            ClientRequest::ReadResourceRequest(req)
        }
        ClientRequest::ListToolsRequest(mut req) => {
            req.extensions = inject_session_context_into_extensions(
                req.extensions,
                session_id,
                working_dir,
                None,
            );
            ClientRequest::ListToolsRequest(req)
        }
        ClientRequest::CallToolRequest(mut req) => {
            req.extensions = inject_session_context_into_extensions(
                req.extensions,
                session_id,
                working_dir,
                tool_call_request_id,
            );
            ClientRequest::CallToolRequest(req)
        }
        ClientRequest::ListPromptsRequest(mut req) => {
            req.extensions = inject_session_context_into_extensions(
                req.extensions,
                session_id,
                working_dir,
                None,
            );
            ClientRequest::ListPromptsRequest(req)
        }
        ClientRequest::GetPromptRequest(mut req) => {
            req.extensions = inject_session_context_into_extensions(
                req.extensions,
                session_id,
                working_dir,
                None,
            );
            ClientRequest::GetPromptRequest(req)
        }
        other => other,
    }
}
