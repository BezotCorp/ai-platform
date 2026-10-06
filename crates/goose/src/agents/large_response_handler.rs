use crate::config::Config;
use rmcp::model::{CallToolResult, ContentBlock, ErrorData};
use std::io::Write;
const DEFAULT_LARGE_TEXT_THRESHOLD: usize = 200_000;

pub(crate) fn max_tool_response_size() -> usize {
    Config::global()
        .get_param::<usize>("GOOSE_MAX_TOOL_RESPONSE_SIZE")
        .unwrap_or(DEFAULT_LARGE_TEXT_THRESHOLD)
}

/// Process tool response and handle large text content
pub fn process_tool_response(
    response: Result<CallToolResult, ErrorData>,
) -> Result<CallToolResult, ErrorData> {
    let threshold = max_tool_response_size();
    match response {
        Ok(mut result) => {
            let mut processed_contents = Vec::new();

            for content in result.content {
                match content.as_text() {
                    Some(text_content) => {
                        // Check if text exceeds threshold
                        if text_content.text.chars().count() > threshold {
                            // Write to temp file
                            match write_large_text_to_file(&text_content.text) {
                                Ok(file_path) => {
                                    // Create a new text content with reference to the file
                                    let message = format!(
                                        "The response returned from the tool call was larger ({} characters) and is stored in the file which you can use other tools to examine or search in: {}",
                                        text_content.text.chars().count(),
                                        file_path
                                    );
                                    processed_contents.push(ContentBlock::text(message));
                                }
                                Err(e) => {
                                    // If file writing fails, include original content with warning
                                    let warning = format!(
                                        "Warning: Failed to write large response to file: {}. Showing full content instead.\n\n{}",
                                        e, text_content.text
                                    );
                                    processed_contents.push(ContentBlock::text(warning));
                                }
                            }
                        } else {
                            // Keep original content for smaller texts
                            processed_contents.push(content);
                        }
                    }
                    None => {
                        // Pass through other content types unchanged
                        processed_contents.push(content);
                    }
                }
            }

            result.content = processed_contents;
            Ok(result)
        }
        Err(e) => Err(e),
    }
}

/// Write large text content to a temporary file
fn write_large_text_to_file(content: &str) -> Result<String, std::io::Error> {
    let mut file = tempfile::Builder::new()
        .prefix("goose_mcp_response_")
        .suffix(".txt")
        .tempfile()?;
    file.write_all(content.as_bytes())?;
    let (_, file_path) = file.keep()?;

    Ok(file_path.to_string_lossy().to_string())
}
