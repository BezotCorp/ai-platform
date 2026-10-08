#[cfg(feature = "aws-providers")]
pub mod bedrock;
pub mod gcpvertexai;
pub mod google {
    use crate::config::Config;
    use anyhow::Result;
    use bcaip_provider_types::conversations::Message;
    use bcaip_provider_types::formats::create_request_with_thinking_budget;
    use bcaip_provider_types::model::ModelConfig;
    use rmcp::model::Tool;
    use serde_json::Value;
    pub fn create_request(
        model_config: &ModelConfig,
        system: &str,
        messages: &[Message],
        tools: &[Tool],
    ) -> Result<Value> {
        // TODO: Remove this config fallback wrapper once gemini_oauth and Vertex/GCP Gemini
        // move into bcaip-providers and receive provider config during construction.
        let thinking_budget = Config::global().get_param("GEMINI25_THINKING_BUDGET").ok();
        create_request_with_thinking_budget(model_config, system, messages, tools, thinking_budget)
    }
}
