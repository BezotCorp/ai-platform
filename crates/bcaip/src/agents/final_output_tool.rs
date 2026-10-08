use crate::{agents::tool_execution::ToolCallResult, recipe::Response};
use indoc::formatdoc;
use rmcp::model::{
    CallToolRequestParams, ContentBlock, ErrorCode, ErrorData, Tool, ToolAnnotations,
};
use serde_json::Value;
use std::borrow::Cow;
pub const FINAL_OUTPUT_TOOL_NAME: &str = "recipe__final_output";
pub const FINAL_OUTPUT_SUCCESS_MESSAGE: &str = "Final output successfully collected.";
pub const FINAL_OUTPUT_CONTINUATION_MESSAGE: &str =
    "You MUST call the `final_output` tool NOW with the final output for the user.";

pub(crate) fn structured_output_unsupported_message(provider_name: &str) -> String {
    format!(
        "This recipe declares a structured `response`, but provider `{provider_name}` can't \
         support it because it never receives bcaip's built-in `final_output` tool, so the \
         model can never satisfy this recipe. Remove the entire `response` block from the recipe \
         or run it with a different provider."
    )
}

pub struct FinalOutputTool {
    pub response: Response,
    /// The final output collected for the user. It will be a single line string for easy script extraction from output.
    pub final_output: Option<String>,
}

impl FinalOutputTool {
    pub fn try_new(response: Response) -> Result<Self, String> {
        let schema_value = response
            .json_schema
            .as_ref()
            .ok_or_else(|| "json_schema is required".to_string())?;
        let schema = schema_value
            .as_object()
            .ok_or_else(|| "json_schema must be an object".to_string())?;
        if schema.is_empty() {
            return Err("empty json_schema is not allowed".to_string());
        }
        jsonschema::validator_for(schema_value).map_err(|error| error.to_string())?;

        Ok(Self {
            response,
            final_output: None,
        })
    }

    pub fn tool(&self) -> Tool {
        let instructions = formatdoc! {r#"
            The final_output tool collects the final output for the user and provides validation for structured JSON final output against a predefined schema.

            This final_output tool MUST be called with the final output for the user.

            Purpose:
            - Collects the final output for the user
            - Ensures that final outputs conform to the expected JSON structure
            - Provides clear validation feedback when outputs don't match the schema

            Usage:
            - Call the `final_output` tool with your JSON final output passed as the argument.

            The expected JSON schema format is:

            {}

            When validation fails, you'll receive:
            - Specific validation errors
            - The expected format
        "#, serde_json::to_string_pretty(self.response.json_schema.as_ref().unwrap()).unwrap()};

        Tool::new(
            FINAL_OUTPUT_TOOL_NAME.to_string(),
            instructions,
            self.response
                .json_schema
                .as_ref()
                .unwrap()
                .as_object()
                .unwrap()
                .clone(),
        )
        .annotate(
            ToolAnnotations::with_title("Final Output".to_string())
                .read_only(false)
                .destructive(false)
                .idempotent(true)
                .open_world(false),
        )
    }

    pub fn system_prompt(&self) -> String {
        formatdoc! {r#"
            # Final Output Instructions

            You MUST use the `final_output` tool to collect the final output for the user rather than providing the output directly in your response.
            The final output MUST be a valid JSON object that is provided to the `final_output` tool when called and it must match the following schema:

            {}

            ----
        "#, serde_json::to_string_pretty(self.response.json_schema.as_ref().unwrap()).unwrap()}
    }

    async fn validate_json_output(&self, output: &Value) -> Result<Value, String> {
        let compiled_schema =
            match jsonschema::validator_for(self.response.json_schema.as_ref().unwrap()) {
                Ok(schema) => schema,
                Err(e) => {
                    return Err(format!("Internal error: Failed to compile schema: {}", e));
                }
            };

        let validation_errors: Vec<String> = compiled_schema
            .iter_errors(output)
            .map(|error| format!("- {}: {}", error.instance_path(), error))
            .collect();

        if validation_errors.is_empty() {
            Ok(output.clone())
        } else {
            Err(format!(
                "Validation failed:\n{}\n\nExpected format:\n{}\n\nPlease correct your output to match the expected JSON schema and try again.",
                validation_errors.join("\n"),
                serde_json::to_string_pretty(self.response.json_schema.as_ref().unwrap())
                    .unwrap_or_else(|_| "Invalid schema".to_string())
            ))
        }
    }

    pub async fn execute_tool_call(&mut self, tool_call: CallToolRequestParams) -> ToolCallResult {
        match tool_call.name.to_string().as_str() {
            FINAL_OUTPUT_TOOL_NAME => {
                let result = self.validate_json_output(&tool_call.arguments.into()).await;
                match result {
                    Ok(parsed_value) => {
                        self.final_output = Some(Self::parsed_final_output_string(parsed_value));
                        ToolCallResult::from(Ok(rmcp::model::CallToolResult::success(vec![
                            ContentBlock::text(FINAL_OUTPUT_SUCCESS_MESSAGE.to_string()),
                        ])))
                    }
                    Err(error) => ToolCallResult::from(Err(ErrorData {
                        code: ErrorCode::INVALID_PARAMS,
                        message: Cow::from(error),
                        data: None,
                    })),
                }
            }
            _ => ToolCallResult::from(Err(ErrorData {
                code: ErrorCode::INVALID_REQUEST,
                message: Cow::from(format!("Unknown tool: {}", tool_call.name)),
                data: None,
            })),
        }
    }

    // Formats the parsed JSON as a single line string so its easy to extract from the output
    fn parsed_final_output_string(parsed_json: Value) -> String {
        serde_json::to_string(&parsed_json).unwrap()
    }
}
