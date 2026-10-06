use crate::security::{SecurityManager, SecurityResult};
use crate::tool_inspection::{InspectionAction, InspectionResult, ToolInspector};
use anyhow::Result;
use async_trait::async_trait;
use bcaip_provider_types::conversations::{Message, ToolRequest};
use bcaip_provider_types::goose_mode::GooseMode;

/// Security inspector that uses pattern matching to detect malicious tool calls
pub struct SecurityInspector {
    security_manager: SecurityManager,
}

impl SecurityInspector {
    pub fn new() -> Self {
        Self {
            security_manager: SecurityManager::new(),
        }
    }

    /// Convert SecurityResult to InspectionResult
    fn convert_security_result(
        &self,
        security_result: &SecurityResult,
        tool_request_id: String,
    ) -> InspectionResult {
        let action = if security_result.is_malicious && security_result.should_ask_user {
            InspectionAction::RequireApproval(Some(format!(
                "🔒 Security Alert\n\n\
                {}\n\n\
                Finding ID: {}",
                security_result.explanation, security_result.finding_id
            )))
        } else {
            InspectionAction::Allow
        };

        InspectionResult {
            tool_request_id,
            action,
            reason: security_result.explanation.clone(),
            confidence: security_result.confidence,
            inspector_name: self.name().to_string(),
            finding_id: Some(security_result.finding_id.clone()),
        }
    }
}

#[async_trait]
impl ToolInspector for SecurityInspector {
    fn name(&self) -> &'static str {
        "security"
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    async fn inspect(
        &self,
        _session_id: &str,
        tool_requests: &[ToolRequest],
        messages: &[Message],
        _goose_mode: GooseMode,
    ) -> Result<Vec<InspectionResult>> {
        let security_results = self
            .security_manager
            .analyze_tool_requests(tool_requests, messages)
            .await?;

        // Convert security results to inspection results
        // The SecurityManager already handles the correlation between tool requests and results
        let inspection_results = security_results
            .into_iter()
            .map(|security_result| {
                let tool_request_id = security_result.tool_request_id.clone();
                self.convert_security_result(&security_result, tool_request_id)
            })
            .collect();

        Ok(inspection_results)
    }

    fn is_enabled(&self) -> bool {
        self.security_manager
            .is_prompt_injection_detection_enabled()
    }
}

impl Default for SecurityInspector {
    fn default() -> Self {
        Self::new()
    }
}
