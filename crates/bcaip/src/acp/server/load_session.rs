use super::message_meta::{
    content_chunk_for_message, merge_message_meta, populate_output_token_limit_content,
};
use crate::acp::server::tool_calls::tool_chain_summary;

use crate::acp::response_builder::{
    agent_thinking_effort_support, build_session_setup_config, session_response_meta,
};
use crate::acp::server::server_informations::{
    ActiveRunDropGuard, BcaipAcpAgent, PendingToolPermission, ResultExt, SessionAgentTarget,
    effective_session_cwd, message_usage_update, resume_saved_provider_session,
    validate_absolute_cwd,
};
use crate::acp::server::tool_calls::conversion::{
    build_initial_tool_call_with_message_meta, tool_call_update_fields_from_response,
    trusted_update_meta,
};
use crate::acp::tool_call_notifier::ToolCallNotifier;
use crate::agents::state_machine::{
    has_unapplied_tool_confirmation_response, pending_tool_confirmations,
};
use crate::agents::{Agent, SessionConfig};
use crate::session::Session;
use agent_client_protocol::schema::v1::{
    Annotations, ContentBlock, ImageContent, LoadSessionRequest, LoadSessionResponse, Meta,
    SessionId, SessionNotification, SessionUpdate, TextContent, ToolCall, ToolCallId,
    ToolCallUpdate,
};
use agent_client_protocol::{Client, ConnectionTo};
use bcaip_provider_types::conversations::{
    Conversation, Message, MessageContent, ToolConfirmationRequest, ToolRequest,
};
use bcaip_sdk_types::custom_notifications::{BcaipSessionNotification, BcaipSessionUpdate};
use rmcp::model::Role;
use std::{collections::HashMap, sync::Arc};
use tokio_util::sync::CancellationToken;
use tracing::{debug, warn};
use uuid::Uuid;
fn replay_audience_annotations(audience: &[Role]) -> Annotations {
    Annotations::new().audience(
        audience
            .iter()
            .map(|role| match role {
                Role::Assistant => agent_client_protocol::schema::v1::Role::Assistant,
                Role::User => agent_client_protocol::schema::v1::Role::User,
            })
            .collect::<Vec<_>>(),
    )
}

fn messages_for_acp_replay(conversation: &Conversation) -> Vec<Message> {
    conversation
        .messages()
        .iter()
        .filter(|message| message.is_user_visible())
        .map(Message::user_visible_content)
        .map(|mut message| {
            populate_output_token_limit_content(&mut message);
            message
        })
        .filter(|message| !message.content.is_empty())
        .collect()
}

fn send_replay_content_chunk(
    cx: &ConnectionTo<Client>,
    session_id: &SessionId,
    message: &Message,
    content: ContentBlock,
) -> std::result::Result<(), agent_client_protocol::Error> {
    let chunk = content_chunk_for_message(message, content);
    let update = match message.role {
        Role::User => SessionUpdate::UserMessageChunk(chunk),
        Role::Assistant => SessionUpdate::AgentMessageChunk(chunk),
    };
    cx.send_notification(SessionNotification::new(session_id.clone(), update))
}

fn build_replayed_tool_call(
    tool_request: &ToolRequest,
    message: &Message,
    client_requests_tool_call_label_enrichment: bool,
) -> ToolCall {
    let mut tool_call = build_initial_tool_call_with_message_meta(
        tool_request,
        message,
        client_requests_tool_call_label_enrichment,
    );

    if !client_requests_tool_call_label_enrichment {
        return tool_call;
    }

    let Some(chain_summary) = tool_request.generated_chain_summary() else {
        return tool_call;
    };
    let bcaip_meta = tool_call
        .meta
        .get_or_insert_default()
        .entry("bcaip".to_string())
        .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
    if !bcaip_meta.is_object() {
        *bcaip_meta = serde_json::Value::Object(serde_json::Map::new());
    }
    bcaip_meta
        .as_object_mut()
        .expect("bcaip metadata was initialized as an object")
        .extend([tool_chain_summary(&chain_summary)]);

    tool_call
}

/// Where to start replaying so that at most roughly `tail` trailing messages
/// are sent without splitting a turn: walk backwards from `len - tail` to the
/// nearest turn boundary (a visible user message that is not a tool response),
/// so tool request/response pairs are never separated. Returns 0 (full
/// replay) when the history is short enough or no boundary exists.
fn replay_start_index(messages: &[Message], tail: usize) -> usize {
    if tail == 0 || messages.len() <= tail {
        return 0;
    }
    let candidate = messages.len() - tail;
    messages[..=candidate]
        .iter()
        .rposition(|message| message.role == Role::User && !message.is_tool_response())
        .unwrap_or(0)
}

fn replay_tail_from_meta(meta: Option<&Meta>) -> Option<usize> {
    meta.and_then(|m| m.get("replayTail"))
        .and_then(|v| v.as_u64())
        .map(|v| v as usize)
}

fn replay_conversation_to_client(
    cx: &ConnectionTo<Client>,
    session: &Session,
    supports_bcaip_custom_notifications: bool,
    client_requests_tool_call_label_enrichment: bool,
    replay_tail: Option<usize>,
) -> Result<usize, agent_client_protocol::Error> {
    let session_id = SessionId::new(session.id.clone());
    let tool_call_notifier = ToolCallNotifier::new(cx, &session_id);

    let messages = session
        .conversation
        .as_ref()
        .map(messages_for_acp_replay)
        .unwrap_or_default();
    let skipped = replay_tail
        .map(|tail| replay_start_index(&messages, tail))
        .unwrap_or(0);
    let messages = &messages[skipped..];

    let mut replay_tool_requests = HashMap::new();

    for message in messages {
        for content_item in &message.content {
            match content_item {
                MessageContent::Text(text) => {
                    let mut tc = TextContent::new(text.text.clone());
                    if let Some(audience) =
                        text.annotations.as_ref().and_then(|a| a.audience.as_ref())
                    {
                        tc = tc.annotations(replay_audience_annotations(audience));
                    }
                    send_replay_content_chunk(cx, &session_id, message, ContentBlock::Text(tc))?;
                }
                MessageContent::Image(image) => {
                    let mut image_content =
                        ImageContent::new(image.data.clone(), image.mime_type.clone());
                    if let Some(audience) =
                        image.annotations.as_ref().and_then(|a| a.audience.as_ref())
                    {
                        image_content =
                            image_content.annotations(replay_audience_annotations(audience));
                    }
                    send_replay_content_chunk(
                        cx,
                        &session_id,
                        message,
                        ContentBlock::Image(image_content),
                    )?;
                }
                MessageContent::ToolRequest(tool_request) => {
                    replay_tool_requests.insert(tool_request.id.clone(), tool_request.clone());

                    let tool_call = build_replayed_tool_call(
                        tool_request,
                        message,
                        client_requests_tool_call_label_enrichment,
                    );

                    tool_call_notifier.send_initial(tool_call)?;
                }
                MessageContent::ToolResponse(tool_response) => {
                    let fields = tool_call_update_fields_from_response(
                        tool_response,
                        replay_tool_requests.get(&tool_response.id),
                        true,
                    );
                    let meta = trusted_update_meta(tool_response).unwrap_or_default();

                    let update =
                        ToolCallUpdate::new(ToolCallId::new(tool_response.id.clone()), fields)
                            .meta(merge_message_meta(meta, message));
                    tool_call_notifier.send_update(update)?;
                }
                MessageContent::Thinking(thinking) => {
                    cx.send_notification(SessionNotification::new(
                        session_id.clone(),
                        SessionUpdate::AgentThoughtChunk(content_chunk_for_message(
                            message,
                            ContentBlock::Text(TextContent::new(thinking.thinking.clone())),
                        )),
                    ))?;
                }
                MessageContent::Error(error) => {
                    send_replay_content_chunk(
                        cx,
                        &session_id,
                        message,
                        ContentBlock::Text(TextContent::new(error.message.clone())),
                    )?;
                }
                MessageContent::SystemNotification(_) => {}
                _ => {}
            }
        }

        if supports_bcaip_custom_notifications && let Some(usage) = &message.metadata.usage {
            cx.send_notification(BcaipSessionNotification {
                session_id: session.id.clone(),
                update: BcaipSessionUpdate::MessageUsage(message_usage_update(
                    message.id.clone(),
                    usage,
                )),
            })?;
        }
    }

    Ok(skipped)
}

impl BcaipAcpAgent {
    fn resend_pending_tool_permissions(
        &self,
        cx: &ConnectionTo<Client>,
        agent: &Arc<Agent>,
        session_id: &str,
        requests: &[ToolConfirmationRequest],
        cancel_token: Option<CancellationToken>,
    ) -> Result<(), agent_client_protocol::Error> {
        let acp_session_id = SessionId::new(session_id.to_string());
        for request in requests {
            self.handle_tool_permission_request(
                cx,
                &acp_session_id,
                PendingToolPermission {
                    request_id: request.id.clone(),
                    tool_name: request.tool_name.clone(),
                    arguments: request.arguments.clone(),
                    prompt: request.prompt.clone(),
                },
                SessionAgentTarget {
                    agent: agent.clone(),
                    session_id: session_id.to_string(),
                    cancel_token: cancel_token.clone(),
                },
            )?;
        }

        Ok(())
    }

    async fn start_resumed_state_machine_turn(
        self: &Arc<Self>,
        cx: &ConnectionTo<Client>,
        agent: &Arc<Agent>,
        session_id: &str,
        requests: &[ToolConfirmationRequest],
    ) -> Result<(), agent_client_protocol::Error> {
        let run_id = format!("resume_{}", Uuid::new_v4());
        let cancel_token = CancellationToken::new();
        self.start_active_run(
            session_id,
            run_id.clone(),
            cancel_token.clone(),
            agent.clone(),
        )
        .await?;

        let acp_session_id = SessionId::new(session_id.to_string());
        if let Err(error) = Self::send_active_run_update(cx, &acp_session_id, Some(&run_id)) {
            self.clear_active_run(session_id, &run_id).await;
            return Err(error);
        }

        let session_config = SessionConfig {
            id: session_id.to_string(),
            schedule_id: None,
            max_turns: None,
            retry_config: None,
        };
        let stream = match agent
            .resume_state_machine_turn(session_config, cancel_token.clone())
            .await
        {
            Ok(Some(stream)) => stream,
            Ok(None) => {
                self.clear_active_run(session_id, &run_id).await;
                Self::send_active_run_update(cx, &acp_session_id, None)?;
                return Ok(());
            }
            Err(error) => {
                self.clear_active_run(session_id, &run_id).await;
                let _ = Self::send_active_run_update(cx, &acp_session_id, None);
                return Err(agent_client_protocol::Error::internal_error().data(format!(
                    "Failed to resume pending tool confirmation: {error}"
                )));
            }
        };

        let server = Arc::clone(self);
        let task_cx = cx.clone();
        let task_agent = agent.clone();
        let task_session_id = session_id.to_string();
        let task_run_id = run_id.clone();
        let task_cancel_token = cancel_token.clone();
        let task_acp_session_id = acp_session_id.clone();
        if let Err(error) = cx.spawn(async move {
            let _run_guard = ActiveRunDropGuard {
                registry: server.active_runs().clone(),
                session_id: task_session_id.clone(),
                run_id: task_run_id.clone(),
                cancel_token: task_cancel_token.clone(),
            };
            let result = server
                .forward_agent_stream(
                    &task_cx,
                    &task_acp_session_id,
                    &task_session_id,
                    &task_agent,
                    &task_cancel_token,
                    stream,
                )
                .await;
            if result.is_ok()
                && let Err(error) = server
                    .send_session_usage_updates(
                        &task_cx,
                        &task_acp_session_id,
                        &task_session_id,
                        &task_agent,
                        &mut None,
                    )
                    .await
            {
                warn!(
                    session_id = task_session_id,
                    ?error,
                    "Failed to update usage after resumed ACP turn"
                );
            }
            server
                .clear_active_run(&task_session_id, &task_run_id)
                .await;
            if let Err(error) = Self::send_active_run_update(&task_cx, &task_acp_session_id, None) {
                warn!(
                    session_id = task_session_id,
                    ?error,
                    "Failed to clear resumed ACP run status"
                );
            }
            if let Err(error) = result {
                warn!(
                    session_id = task_session_id,
                    ?error,
                    "Resumed ACP state-machine turn failed"
                );
            }
            Ok(())
        }) {
            cancel_token.cancel();
            self.clear_active_run(session_id, &run_id).await;
            let _ = Self::send_active_run_update(cx, &SessionId::new(session_id), None);
            return Err(error);
        }

        if let Err(error) = self.resend_pending_tool_permissions(
            cx,
            agent,
            session_id,
            requests,
            Some(cancel_token.clone()),
        ) {
            cancel_token.cancel();
            self.clear_active_run(session_id, &run_id).await;
            let _ = Self::send_active_run_update(cx, &acp_session_id, None);
            return Err(error);
        }

        Ok(())
    }

    pub(super) async fn handle_load_session(
        self: &Arc<Self>,
        cx: &ConnectionTo<Client>,
        args: LoadSessionRequest,
    ) -> Result<LoadSessionResponse, agent_client_protocol::Error> {
        debug!(?args, "load session request");

        let session_id_str = args.session_id.0.to_string();

        let mut session = self
            .session_manager()
            .get_session(&session_id_str, true)
            .await
            .map_err(|_| {
                agent_client_protocol::Error::resource_not_found(Some(session_id_str.clone()))
                    .data(format!("Session not found: {}", session_id_str))
            })?;

        let cwd = effective_session_cwd(self.session_cwd(), &args.cwd);
        validate_absolute_cwd(&cwd)?;

        session = self
            .prepare_session_for_activation(session, cwd, args.mcp_servers, true)
            .await?;

        let replayed_from = replay_conversation_to_client(
            cx,
            &session,
            self.supports_bcaip_custom_notifications(),
            self.requests_tool_call_label_enrichment(),
            replay_tail_from_meta(args.meta.as_ref()),
        )?;
        let (agent, extension_results) = self.prepare_acp_session_agent(cx, &session).await?;
        self.apply_session_recipe(&agent, &session).await?;
        self.register_acp_session(session_id_str.clone(), agent.clone())
            .await;
        let provider = agent
            .provider()
            .await
            .internal_err_ctx("Failed to get provider while loading ACP session")?;
        resume_saved_provider_session(&provider, session.conversation.as_ref()).await;
        session = self
            .session_manager()
            .get_session(&session_id_str, false)
            .await
            .internal_err_ctx("Failed to reload session")?;

        agent
            .extension_manager
            .update_working_dir(&session.working_dir)
            .await;

        let (mode_state, config_options) = build_session_setup_config(
            self.provider_inventory(),
            &session,
            &agent_thinking_effort_support(&agent).await,
        )
        .await?;

        let mut response = LoadSessionResponse::new().modes(mode_state);
        if let Some(co) = config_options {
            response = response.config_options(co);
        }

        let mut meta = session_response_meta(&session, &extension_results);
        if replayed_from > 0 {
            meta.insert(
                "replaySkipped".to_string(),
                serde_json::Value::Number(replayed_from.into()),
            );
        }
        response = response.meta(meta);

        let pending_confirmations = session
            .conversation
            .as_ref()
            .map(pending_tool_confirmations)
            .unwrap_or_default();
        let should_resume_state_machine = crate::agents::state_machine::enabled()
            && (!pending_confirmations.is_empty()
                || session
                    .conversation
                    .as_ref()
                    .is_some_and(has_unapplied_tool_confirmation_response));
        if should_resume_state_machine {
            self.start_resumed_state_machine_turn(
                cx,
                &agent,
                &session_id_str,
                &pending_confirmations,
            )
            .await?;
        } else {
            self.resend_pending_tool_permissions(
                cx,
                &agent,
                &session_id_str,
                &pending_confirmations,
                None,
            )?;
        }

        self.closed_session_ids()
            .lock()
            .await
            .remove(&session_id_str);
        Ok(response)
    }
}
