use crate::conversations::message::{
    TOOL_META_CHAIN_SUMMARY_KEY, TOOL_META_EXTERNAL_DISPATCH_KEY, TOOL_META_PROVIDER_INDEX_KEY,
    TOOL_META_TITLE_KEY, ToolChainSummary, ToolNameParts, ToolRequest,
};

impl<'a> From<&'a str> for ToolNameParts<'a> {
    fn from(name: &'a str) -> Self {
        match name.split_once("__") {
            Some((extension_name, tool_name)) => Self {
                extension_name: Some(extension_name),
                tool_name,
            },
            None => Self {
                extension_name: None,
                tool_name: name,
            },
        }
    }
}

impl ToolRequest {
    pub fn tool_name_parts(&self) -> Option<ToolNameParts<'_>> {
        let tool_call = self.tool_call.as_ref().ok()?;
        let name = tool_call.name.as_ref();
        let mut parts = ToolNameParts::from(name);
        if let Some(extension_name) = self
            .tool_meta
            .as_ref()
            .and_then(|meta| meta.get("bcaip_extension"))
            .and_then(serde_json::Value::as_str)
        {
            parts.extension_name = Some(extension_name);
            parts.tool_name = name
                .strip_prefix(extension_name)
                .and_then(|name| name.strip_prefix("__"))
                .unwrap_or(parts.tool_name);
        }
        Some(parts)
    }

    pub fn to_readable_string(&self) -> String {
        match &self.tool_call {
            Ok(tool_call) => {
                format!(
                    "Tool: {}, Args: {}",
                    tool_call.name,
                    serde_json::to_string_pretty(&tool_call.arguments)
                        .unwrap_or_else(|_| "<<invalid json>>".to_string())
                )
            }
            Err(e) => format!("Invalid tool call: {}", e),
        }
    }

    /// Returns true if this tool request was already executed externally
    /// (e.g. by an ACP provider's underlying SDK) and the agent loop must
    /// not redispatch it. See [`TOOL_META_EXTERNAL_DISPATCH_KEY`].
    pub fn was_executed_externally(&self) -> bool {
        self.tool_meta
            .as_ref()
            .and_then(|v| v.get(TOOL_META_EXTERNAL_DISPATCH_KEY))
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
    }

    pub fn generated_title(&self) -> Option<&str> {
        self.tool_meta
            .as_ref()
            .and_then(|v| v.get(TOOL_META_TITLE_KEY))
            .and_then(|v| v.as_str())
    }

    /// Provider-reported index of this tool call within the streamed response.
    /// See [`TOOL_META_PROVIDER_INDEX_KEY`].
    pub fn provider_index(&self) -> Option<i32> {
        self.tool_meta
            .as_ref()
            .and_then(|v| v.get(TOOL_META_PROVIDER_INDEX_KEY))
            .and_then(|v| v.as_i64())
            .map(|index| index as i32)
    }

    pub fn generated_chain_summary(&self) -> Option<ToolChainSummary> {
        let obj = self
            .tool_meta
            .as_ref()
            .and_then(|v| v.get(TOOL_META_CHAIN_SUMMARY_KEY))?;
        let summary = obj.get("summary").and_then(|v| v.as_str())?.to_string();
        let count = obj.get("count").and_then(|v| v.as_u64())?;
        if count == 0 {
            return None;
        }
        Some(ToolChainSummary {
            summary,
            count: count as usize,
        })
    }
}
