use bcaip_provider_types::conversations::{MessageContent, ToolRequest};

pub(crate) fn breaks_consecutive_tool_calls(content: &MessageContent) -> bool {
    matches!(
        content,
        MessageContent::Text(_) | MessageContent::Thinking(_) | MessageContent::Image(_)
    )
}

#[derive(Debug)]
struct ToolChainStep {
    request: ToolRequest,
    responded: bool,
}

#[derive(Debug)]
struct TrackedToolChain {
    steps: Vec<ToolChainStep>,
}

impl TrackedToolChain {
    fn new(request: ToolRequest) -> Self {
        Self {
            steps: vec![ToolChainStep {
                request,
                responded: false,
            }],
        }
    }

    fn add_request(&mut self, request: ToolRequest) {
        self.steps.push(ToolChainStep {
            request,
            responded: false,
        });
    }

    fn contains(&self, tool_call_id: &str) -> bool {
        self.steps
            .iter()
            .any(|step| step.request.id == tool_call_id)
    }

    fn mark_responded(&mut self, tool_call_id: &str) {
        let Some(step) = self
            .steps
            .iter_mut()
            .find(|step| step.request.id == tool_call_id)
        else {
            return;
        };

        step.responded = true;
    }

    fn is_complete(&self) -> bool {
        self.steps.iter().all(|step| step.responded)
    }

    fn into_ready(self) -> ReadyToolChain {
        ReadyToolChain {
            tool_requests: self.steps.into_iter().map(|step| step.request).collect(),
        }
    }
}

pub(crate) struct ReadyToolChain {
    pub(crate) tool_requests: Vec<ToolRequest>,
}

/// Tracks tool-chain membership and readiness for one ACP prompt stream.
#[derive(Default)]
pub(crate) struct ToolChainTracker {
    current_chain: Option<TrackedToolChain>,
    waiting_chains: Vec<TrackedToolChain>,
}

impl ToolChainTracker {
    pub(crate) fn record_request(&mut self, request: ToolRequest) {
        if let Some(chain) = &mut self.current_chain {
            chain.add_request(request);
        } else {
            self.current_chain = Some(TrackedToolChain::new(request));
        }
    }

    pub(crate) fn record_response(&mut self, tool_call_id: &str) -> Option<ReadyToolChain> {
        if let Some(current_chain) = &mut self.current_chain
            && current_chain.contains(tool_call_id)
        {
            current_chain.mark_responded(tool_call_id);
            return None;
        }

        let waiting_chain_index = self
            .waiting_chains
            .iter()
            .position(|chain| chain.contains(tool_call_id))?;

        let waiting_chain = &mut self.waiting_chains[waiting_chain_index];
        waiting_chain.mark_responded(tool_call_id);

        if !waiting_chain.is_complete() {
            return None;
        }

        let ready_chain = self.waiting_chains.remove(waiting_chain_index);
        Some(ready_chain.into_ready())
    }

    pub(crate) fn close_current_chain(&mut self) -> Option<ReadyToolChain> {
        let chain = self.current_chain.take()?;
        if chain.steps.len() < 2 {
            return None;
        }

        if !chain.is_complete() {
            self.waiting_chains.push(chain);
            return None;
        }

        Some(chain.into_ready())
    }
}
