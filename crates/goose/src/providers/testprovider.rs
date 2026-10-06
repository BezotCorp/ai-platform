use anyhow::{Result, anyhow};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::{Arc, Mutex};
use std::{collections::HashMap, fs, path::Path};

use super::base::ProviderDef;
use crate::utils::bytes_to_hex;
use bcaip_provider_types::base::{MessageStream, Provider, ProviderMetadata};
use bcaip_provider_types::conversations::ProviderUsage;
use bcaip_provider_types::conversations::{Message, ToolResponse};
use bcaip_provider_types::errors::ProviderError;
use bcaip_provider_types::model::ModelConfig;
use futures::future::BoxFuture;
use rmcp::model::{CallToolResult, Tool};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TestInput {
    system: String,
    messages: Vec<Message>,
    tools: Vec<Tool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TestOutput {
    message: Message,
    usage: ProviderUsage,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TestRecord {
    input: TestInput,
    output: TestOutput,
}

pub struct TestProvider {
    inner: Option<Arc<dyn Provider>>,
    records: Arc<Mutex<HashMap<String, TestRecord>>>,
    file_path: String,
    name: String,
}

impl TestProvider {
    const PROVIDER_NAME: &str = "test";

    pub fn new_recording(inner: Arc<dyn Provider>, file_path: impl Into<String>) -> Self {
        Self {
            inner: Some(inner),
            records: Arc::new(Mutex::new(HashMap::new())),
            file_path: file_path.into(),
            name: Self::PROVIDER_NAME.to_string(),
        }
    }

    pub fn new_replaying(file_path: impl Into<String>) -> Result<Self> {
        let file_path = file_path.into();
        let records = Self::load_records(&file_path)?;

        Ok(Self {
            inner: None,
            records: Arc::new(Mutex::new(records)),
            file_path,
            name: Self::PROVIDER_NAME.to_string(),
        })
    }

    pub fn finish_recording(self) -> Result<()> {
        if self.inner.is_some() {
            self.save_records()?;
        }
        Ok(())
    }

    fn hash_input(messages: &[Message]) -> String {
        use bcaip_provider_types::conversations::MessageContent;
        // Strip internal metadata (e.g. tool_meta/_meta) from content before hashing.
        // This metadata is used for internal routing (like goose_extension ownership)
        // and isn't part of the semantic input the LLM sees, so it shouldn't affect
        // replay matching.
        let stable_messages: Vec<_> = messages
            .iter()
            .map(|msg| {
                let mut cleaned_content: Vec<_> = msg.content.to_vec();

                for content in &mut cleaned_content {
                    match content {
                        MessageContent::ToolRequest(req) => {
                            req.tool_meta = None;
                        }
                        MessageContent::ToolResponse(ToolResponse {
                            tool_result:
                                Ok(
                                    result @ CallToolResult {
                                        is_error: Some(false),
                                        ..
                                    },
                                ),
                            ..
                        }) => {
                            result.is_error = None;
                            result.result_type = None;
                        }
                        _ => {}
                    }
                }
                (msg.role.clone(), cleaned_content)
            })
            .collect();
        let serialized = serde_json::to_string(&stable_messages).unwrap_or_default();
        let mut hasher = Sha256::new();
        hasher.update(serialized.as_bytes());
        bytes_to_hex(hasher.finalize())
    }

    fn load_records(file_path: &str) -> Result<HashMap<String, TestRecord>> {
        if !Path::new(file_path).exists() {
            return Ok(HashMap::new());
        }

        let content = fs::read_to_string(file_path)?;
        let records: HashMap<String, TestRecord> = serde_json::from_str(&content)?;
        Ok(records)
    }

    pub fn save_records(&self) -> Result<()> {
        let records = self.records.lock().unwrap();
        let content = serde_json::to_string_pretty(&*records)?;
        fs::write(&self.file_path, content)?;
        Ok(())
    }

    pub fn get_record_count(&self) -> usize {
        self.records.lock().unwrap().len()
    }
}

impl bcaip_provider_types::base::ProviderDescriptor for TestProvider {
    fn metadata() -> ProviderMetadata {
        ProviderMetadata::new(
            Self::PROVIDER_NAME,
            "Test Provider",
            "Provider for testing that can record/replay interactions",
            "test-model",
            vec!["test-model"],
            "",
            vec![],
        )
    }
}

impl ProviderDef for TestProvider {
    type Provider = Self;

    fn from_env(
        _extensions: Vec<crate::config::ExtensionConfig>,
        _tls_config: Option<goose_providers::api_client::TlsConfig>,
    ) -> BoxFuture<'static, Result<Self::Provider>> {
        Box::pin(async { Err(anyhow!("TestProvider must be constructed explicitly")) })
    }
}

#[async_trait]
impl Provider for TestProvider {
    fn get_name(&self) -> &str {
        &self.name
    }

    async fn stream(
        &self,
        model_config: &ModelConfig,
        system: &str,
        messages: &[Message],
        tools: &[Tool],
    ) -> Result<MessageStream, ProviderError> {
        let hash = Self::hash_input(messages);

        if let Some(inner) = &self.inner {
            // Call inner provider's stream and collect it
            let stream = inner.stream(model_config, system, messages, tools).await?;
            let (message, usage) = bcaip_provider_types::base::collect_stream(stream).await?;

            let record = TestRecord {
                input: TestInput {
                    system: system.to_string(),
                    messages: messages.to_vec(),
                    tools: tools.to_vec(),
                },
                output: TestOutput {
                    message: message.clone(),
                    usage: usage.clone(),
                },
            };

            {
                let mut records = self.records.lock().unwrap();
                records.insert(hash, record);
            }

            Ok(bcaip_provider_types::base::stream_from_single_message(
                message, usage,
            ))
        } else {
            let records = self.records.lock().unwrap();
            if let Some(record) = records.get(&hash) {
                let message = record.output.message.clone();
                let usage = record.output.usage.clone();
                Ok(bcaip_provider_types::base::stream_from_single_message(
                    message, usage,
                ))
            } else {
                Err(ProviderError::ExecutionError(format!(
                    "No recorded response found for input hash: {}",
                    hash
                )))
            }
        }
    }
}
