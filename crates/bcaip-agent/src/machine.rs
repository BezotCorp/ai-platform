use std::{collections::HashSet, sync::Arc};

use crate::operation::{
    ConversationEffect, Emitter, Inference, InferenceInput, MachineEffect, Operation,
    OperationFuture, OperationResult, StepResult,
};
use anyhow::{Result, anyhow};
use async_trait::async_trait;
use bcaip_provider_types::conversations::{Conversation, Usage};
use bcaip_provider_types::maybe_send::{MaybeSend, MaybeSync};
use tokio_util::sync::CancellationToken;

pub trait MachineSession: MaybeSend + MaybeSync {
    fn id(&self) -> &str;
    fn conversation(&self) -> Option<&Conversation>;
}

#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
pub trait SessionLoader<S>: MaybeSend + MaybeSync {
    async fn load(&self, session_id: &str) -> Result<S>;
}

#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
pub trait EffectHandler<S, E>: MaybeSend + MaybeSync {
    async fn apply_effects(&self, session: &S, effects: &mut [E], emit: &Emitter) -> Result<()>;
}

pub trait EffectUsage<E>: MaybeSend + MaybeSync {
    fn usage(&self, _effect: &E) -> Option<Usage> {
        None
    }
}

pub enum Step<'a, S, E = ConversationEffect> {
    Operation(Arc<dyn Operation<S, E> + 'a>),
    Inference(Arc<dyn Inference<S, E> + 'a>),
}

impl<S, E: MaybeSend> Step<'_, S, E> {
    fn operation(&self) -> &dyn Operation<S, E> {
        match self {
            Step::Operation(operation) => operation.as_ref(),
            Step::Inference(inference) => inference.as_ref(),
        }
    }
}

pub struct StateMachine<'a, S, E = ConversationEffect> {
    steps: Vec<Step<'a, S, E>>,
    cancel: CancellationToken,
}

fn add_tools_to_inference_input(
    input: &mut InferenceInput,
    tool_names: &mut HashSet<String>,
    tools: Vec<rmcp::model::Tool>,
) -> Result<()> {
    for tool in tools {
        if !tool_names.insert(tool.name.to_string()) {
            anyhow::bail!("multiple operations registered tool '{}'", tool.name);
        }
        input.tools.push(tool);
    }
    Ok(())
}

impl<'a, S, E> StateMachine<'a, S, E>
where
    S: MachineSession,
    E: MachineEffect + MaybeSend + 'static,
{
    pub fn new(steps: Vec<Step<'a, S, E>>, cancel: CancellationToken) -> Self {
        Self { steps, cancel }
    }

    pub async fn step(&self, session: &S, emit: &Emitter) -> Result<Option<StepResult<E>>> {
        let conversation = session
            .conversation()
            .ok_or_else(|| anyhow!("state-machine session loaded without conversation"))?;

        for step in &self.steps {
            let name = step.operation().name();
            let result = if self.cancel.is_cancelled() {
                OperationResult::NotApplicable
            } else {
                let step_fut: OperationFuture<'_, Result<OperationResult<E>>> = match step {
                    Step::Operation(operation) => operation.run(session, conversation, emit),
                    Step::Inference(inference) => {
                        if !inference.applies(conversation) {
                            continue;
                        }
                        let mut input = InferenceInput::default();
                        let mut tool_names = HashSet::new();
                        for operation in self.steps.iter().map(|step| step.operation()) {
                            let tools = tokio::select! {
                                biased;
                                _ = self.cancel.cancelled() => return Ok(None),
                                tools = operation.inference_tools(session) => tools?,
                            };
                            add_tools_to_inference_input(&mut input, &mut tool_names, tools)?;
                            input
                                .prompt_parts
                                .extend(operation.prompt_parts(session, conversation).await?);
                            input
                                .moim_parts
                                .extend(operation.moim_parts(session, conversation).await?);
                        }
                        inference.infer(session, conversation, input, emit)
                    }
                };
                step_fut.await?
            };
            let cancelled = self.cancel.is_cancelled();
            let result = if cancelled {
                step.operation()
                    .cancel(session, conversation, result, emit)
                    .await?
            } else {
                result
            };

            match result {
                OperationResult::NotApplicable => {}
                OperationResult::Applied(mut result) => {
                    result.applied_step = Some(name);
                    for effect in &mut result.effects {
                        effect.ensure_message_ids();
                    }
                    if cancelled {
                        result.yield_to_client = true;
                    }
                    return Ok(Some(result));
                }
            }
        }

        Ok(None)
    }

    pub async fn apply<R>(
        &self,
        runtime: &R,
        session: &S,
        result: &mut StepResult<E>,
        emit: &Emitter,
    ) -> Result<()>
    where
        R: EffectHandler<S, E>,
    {
        for effect in &mut result.effects {
            effect.ensure_message_ids();
        }
        runtime
            .apply_effects(session, &mut result.effects, emit)
            .await
    }

    pub async fn run<R>(&self, runtime: &R, session_id: &str, emit: &Emitter) -> Result<S>
    where
        R: SessionLoader<S> + EffectHandler<S, E>,
    {
        loop {
            let session = runtime.load(session_id).await?;
            let Some(mut result) = self.step(&session, emit).await? else {
                break;
            };
            self.apply(runtime, &session, &mut result, emit).await?;
            if result.yield_to_client {
                break;
            }
        }
        runtime.load(session_id).await
    }
}
