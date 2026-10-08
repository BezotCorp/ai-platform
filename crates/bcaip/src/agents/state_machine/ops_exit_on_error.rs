//! Ends the turn when an error remains at the end of the conversation.
use crate::agents::state_machine::effects::BcaipEffect;
use crate::session::Session;
use anyhow::Result;
use async_trait::async_trait;
use bcaip_agent::operation::{
    Emitter, Operation, OperationResult, not_applicable, trailing_error, yielded,
};
use bcaip_provider_types::conversations::Conversation;
pub struct ExitOnErrorOperation;

#[async_trait]
impl Operation<Session, BcaipEffect> for ExitOnErrorOperation {
    fn name(&self) -> &'static str {
        "exit_on_error"
    }

    async fn run(
        &self,
        _session: &Session,
        conversation: &Conversation,
        _emit: &Emitter,
    ) -> Result<OperationResult<BcaipEffect>> {
        if trailing_error(conversation).is_none() {
            return not_applicable();
        }

        yielded()
    }
}
