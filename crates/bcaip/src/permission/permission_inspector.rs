use crate::config::PermissionManager;
use crate::permission::permission_judge::{PermissionCheckResult, detect_read_only_requests};
use crate::tool_inspection::{InspectionAction, InspectionResult, ToolInspector};
use crate::{
    agents::{platform_extensions::MANAGE_EXTENSIONS_TOOL_NAME_COMPLETE, types::SharedProvider},
    config::permission::PermissionLevel,
};
use anyhow::Result;
use async_trait::async_trait;
use bcaip_provider_types::bcaip_mode::BcaipMode;
use bcaip_provider_types::conversations::{Message, ToolRequest};
use rmcp::model::Tool;
use std::collections::HashSet;
use std::sync::{Arc, RwLock};

/// Permission Inspector that handles tool permission checking
pub struct PermissionInspector {
    pub permission_manager: Arc<PermissionManager>,
    provider: SharedProvider,
    session_manager: Arc<crate::session::SessionManager>,
    readonly_tools: RwLock<HashSet<String>>,
}

fn cache_non_readonly_decision(
    permission_manager: &PermissionManager,
    candidate: &ToolRequest,
    is_readonly: bool,
) {
    if is_readonly {
        return;
    }
    if let Ok(tool_call) = &candidate.tool_call {
        permission_manager
            .update_smart_approve_permission(&tool_call.name, PermissionLevel::AskBefore);
    }
}

impl PermissionInspector {
    pub fn new(
        permission_manager: Arc<PermissionManager>,
        provider: SharedProvider,
        session_manager: Arc<crate::session::SessionManager>,
    ) -> Self {
        Self {
            permission_manager,
            provider,
            session_manager,
            readonly_tools: RwLock::new(HashSet::new()),
        }
    }

    // readonly_tools is per-agent to avoid concurrent session clobbering; write-annotated
    // tools are cached globally via PermissionManager.
    pub fn apply_tool_annotations(&self, tools: &[Tool]) {
        let mut readonly_annotated = HashSet::new();
        for tool in tools {
            let Some(anns) = &tool.annotations else {
                continue;
            };
            if anns.read_only_hint == Some(true) {
                readonly_annotated.insert(tool.name.to_string());
            }
        }
        *self.readonly_tools.write().unwrap() = readonly_annotated;
        self.permission_manager.apply_tool_annotations(tools);
    }

    pub fn is_readonly_annotated_tool(&self, tool_name: &str) -> bool {
        self.readonly_tools.read().unwrap().contains(tool_name)
    }

    /// Process inspection results into permission decisions
    /// This method takes all inspection results and converts them into a PermissionCheckResult
    /// that can be used by the agent to determine which tools to approve, deny, or ask for approval
    pub fn process_inspection_results(
        &self,
        remaining_requests: &[ToolRequest],
        inspection_results: &[InspectionResult],
    ) -> PermissionCheckResult {
        use crate::tool_inspection::apply_inspection_results_to_permissions;
        // Start with permission inspector's decisions as the baseline
        let mut permission_check_result = PermissionCheckResult {
            approved: vec![],
            needs_approval: vec![],
            denied: vec![],
        };

        // Apply permission inspector results first (baseline behavior)
        let permission_results: Vec<_> = inspection_results
            .iter()
            .filter(|result| result.inspector_name == "permission")
            .collect();

        for request in remaining_requests {
            // Find the permission decision for this request
            if let Some(permission_result) = permission_results
                .iter()
                .find(|result| result.tool_request_id == request.id)
            {
                match permission_result.action {
                    InspectionAction::Allow => {
                        permission_check_result.approved.push(request.clone());
                    }
                    InspectionAction::Deny => {
                        permission_check_result.denied.push(request.clone());
                    }
                    InspectionAction::RequireApproval(_) => {
                        permission_check_result.needs_approval.push(request.clone());
                    }
                }
            } else {
                // If no permission result found, default to needs approval for safety
                permission_check_result.needs_approval.push(request.clone());
            }
        }

        // Apply security and other inspector results as overrides
        let non_permission_results: Vec<_> = inspection_results
            .iter()
            .filter(|result| result.inspector_name != "permission")
            .cloned()
            .collect();

        if !non_permission_results.is_empty() {
            permission_check_result = apply_inspection_results_to_permissions(
                permission_check_result,
                &non_permission_results,
            );
        }

        permission_check_result
    }
}

#[async_trait]
impl ToolInspector for PermissionInspector {
    fn name(&self) -> &'static str {
        "permission"
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    async fn inspect(
        &self,
        session_id: &str,
        tool_requests: &[ToolRequest],
        _messages: &[Message],
        bcaip_mode: BcaipMode,
    ) -> Result<Vec<InspectionResult>> {
        let mut results = Vec::new();
        let permission_manager = &self.permission_manager;
        let mut llm_detect_candidates: Vec<&ToolRequest> = Vec::new();

        for request in tool_requests {
            if let Ok(tool_call) = &request.tool_call {
                let tool_name = &tool_call.name;

                let action = match bcaip_mode {
                    BcaipMode::Chat => continue,
                    BcaipMode::Auto => InspectionAction::Allow,
                    BcaipMode::Approve | BcaipMode::SmartApprove => {
                        // 1. Check user-defined permission first
                        if let Some(level) = permission_manager.get_user_permission(tool_name) {
                            match level {
                                PermissionLevel::AlwaysAllow => InspectionAction::Allow,
                                PermissionLevel::NeverAllow => InspectionAction::Deny,
                                PermissionLevel::AskBefore => {
                                    InspectionAction::RequireApproval(None)
                                }
                            }
                        // 2. Check for a read-only annotation in SmartApprove mode
                        } else if bcaip_mode == BcaipMode::SmartApprove
                            && self.is_readonly_annotated_tool(tool_name)
                        {
                            InspectionAction::Allow
                        // 3. Special case for extension management
                        } else if tool_name == MANAGE_EXTENSIONS_TOOL_NAME_COMPLETE {
                            InspectionAction::RequireApproval(Some(
                                "Extension management requires approval for security".to_string(),
                            ))
                        // 4. Defer to LLM detection (SmartApprove, uncached or legacy cached allow)
                        } else if bcaip_mode == BcaipMode::SmartApprove
                            && matches!(
                                permission_manager.get_smart_approve_permission(tool_name),
                                None | Some(PermissionLevel::AlwaysAllow)
                            )
                        {
                            llm_detect_candidates.push(request);
                            continue;
                        // 5. Default: require approval for unknown tools
                        } else {
                            InspectionAction::RequireApproval(None)
                        }
                    }
                };

                let reason = match &action {
                    InspectionAction::Allow => {
                        if bcaip_mode == BcaipMode::Auto {
                            "Auto mode - all tools approved".to_string()
                        } else if self.is_readonly_annotated_tool(tool_name) {
                            "Tool annotated as read-only".to_string()
                        } else {
                            "User permission allows this tool".to_string()
                        }
                    }
                    InspectionAction::Deny => "User permission denies this tool".to_string(),
                    InspectionAction::RequireApproval(_) => {
                        if tool_name == MANAGE_EXTENSIONS_TOOL_NAME_COMPLETE {
                            "Extension management requires user approval".to_string()
                        } else {
                            "Tool requires user approval".to_string()
                        }
                    }
                };

                results.push(InspectionResult {
                    tool_request_id: request.id.clone(),
                    action,
                    reason,
                    confidence: 1.0, // Permission decisions are definitive
                    inspector_name: self.name().to_string(),
                    finding_id: None,
                });
            }
        }

        // LLM-based read-only detection for deferred SmartApprove candidates
        if !llm_detect_candidates.is_empty() {
            let detected_request_ids: HashSet<String> = match self.provider.lock().await.clone() {
                Some(provider) => detect_read_only_requests(
                    provider,
                    &self.session_manager,
                    session_id,
                    llm_detect_candidates.to_vec(),
                )
                .await
                .into_iter()
                .collect(),
                None => Default::default(),
            };

            for candidate in &llm_detect_candidates {
                let is_readonly = detected_request_ids.contains(&candidate.id);

                cache_non_readonly_decision(permission_manager, candidate, is_readonly);

                results.push(InspectionResult {
                    tool_request_id: candidate.id.clone(),
                    action: if is_readonly {
                        InspectionAction::Allow
                    } else {
                        InspectionAction::RequireApproval(None)
                    },
                    reason: if is_readonly {
                        "LLM detected as read-only".to_string()
                    } else {
                        "Tool requires user approval".to_string()
                    },
                    confidence: 1.0, // Permission decisions are definitive
                    inspector_name: self.name().to_string(),
                    finding_id: None,
                });
            }
        }

        Ok(results)
    }
}
