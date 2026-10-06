use crate::acp::response_builder::{
    agent_thinking_effort_support, build_session_setup_config, session_meta,
};
use crate::acp::server::server_informations::{
    GooseAcpAgent, ResultExt, effective_session_cwd, resume_saved_provider_session,
    validate_absolute_cwd,
};
use agent_client_protocol::schema::v1::{ForkSessionRequest, ForkSessionResponse, Meta, SessionId};
use agent_client_protocol::{Client, ConnectionTo};

impl GooseAcpAgent {
    #[allow(dead_code)]
    pub(super) async fn handle_fork_session(
        &self,
        cx: &ConnectionTo<Client>,
        args: ForkSessionRequest,
    ) -> Result<ForkSessionResponse, agent_client_protocol::Error> {
        let conversation_before = conversation_before_from_meta(args.meta.as_ref())?;
        let source_session_id = &*args.session_id.0;

        let source = self
            .session_manager()
            .get_session(source_session_id, false)
            .await
            .internal_err()?;

        // Resolve and validate the effective cwd before copying anything, so a
        // x request cannot leave a stray "(copy)" session in the store.
        let cwd = effective_session_cwd(self.session_cwd(), &args.cwd);
        validate_absolute_cwd(&cwd)?;

        let fork_name = if source.name.trim().is_empty() {
            "(copy)".to_string()
        } else {
            format!("{} (copy)", source.name)
        };

        let new_session = self
            .session_manager()
            .copy_session(source_session_id, fork_name)
            .await
            .internal_err()?;
        let new_session_id = new_session.id.clone();

        if let Some(conversation_before) = conversation_before {
            self.session_manager()
                .truncate_conversation(&new_session_id, conversation_before)
                .await
                .internal_err()?;
        }

        let new_session = self
            .session_manager()
            .get_session(&new_session_id, true)
            .await
            .internal_err()?;

        let goose_session = self
            .prepare_session_for_activation(new_session.clone(), cwd, args.mcp_servers, true)
            .await?;

        let (agent, extension_results) = self.prepare_acp_session_agent(cx, &goose_session).await?;
        self.apply_session_recipe(&agent, &goose_session).await?;
        self.register_acp_session(goose_session.id.clone(), agent.clone())
            .await;
        let provider = agent
            .provider()
            .await
            .internal_err_ctx("Failed to get provider while forking ACP session")?;
        resume_saved_provider_session(&provider, goose_session.conversation.as_ref()).await;
        let effort_support = agent_thinking_effort_support(&agent).await;

        let acp_session_id = SessionId::new(new_session_id.clone());
        let mut meta = session_meta(&goose_session);
        if let Ok(v) = serde_json::to_value(&extension_results) {
            meta.insert("extensionResults".to_string(), v);
        }

        let (mode_state, config_options) =
            build_session_setup_config(self.provider_inventory(), &goose_session, &effort_support)
                .await?;

        let mut response = ForkSessionResponse::new(acp_session_id.clone())
            .modes(mode_state)
            .meta(meta);

        if let Some(co) = config_options {
            response = response.config_options(co);
        }
        Ok(response)
    }
}

fn conversation_before_from_meta(
    meta: Option<&Meta>,
) -> Result<Option<i64>, agent_client_protocol::Error> {
    let Some(value) = meta.and_then(|meta| meta.get("conversationBefore")) else {
        return Ok(None);
    };
    if value.is_null() {
        return Ok(None);
    }

    value.as_i64().map(Some).ok_or_else(|| {
        agent_client_protocol::Error::invalid_params()
            .data("conversationBefore must be an integer timestamp")
    })
}
