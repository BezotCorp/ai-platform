use crate::finalize_usage;
use crate::llamacpp::inference_engine::{
    GenerationContext, StopSuffixTrimmer, TokenAction, generation_loop, prepare_generation,
};
use crate::{
    native_tool_format::NativeToolFormat, native_tool_parsing::message_from_native_tool_text,
    native_tool_stream_splitter::NativeToolStreamSplitter, thinking_output::ThinkingOutputFilter,
};
use goose_provider_types::conversations::{Message, MessageContent};
use goose_provider_types::errors::ProviderError;
pub(crate) fn generate_with_native_tools(
    ctx: &mut GenerationContext<'_>,
    oai_messages_json: &str,
    full_tools_json: Option<&str>,
    compact_tools: Option<&str>,
) -> Result<(), ProviderError> {
    let prepared = prepare_generation(ctx, oai_messages_json, full_tools_json, compact_tools)?;
    let template_result = prepared.template_result;
    let mut llama_ctx = prepared.llama_ctx;
    let prompt_token_count = prepared.prompt_token_count;
    let effective_ctx = prepared.effective_ctx;

    let message_id = ctx.message_id;
    let tx = ctx.tx;
    let mut generated_text = String::new();
    let mut stop_trimmer = StopSuffixTrimmer::new(&template_result.additional_stops);
    let mut stop_string_emitted = false;

    let mut splitter = NativeToolStreamSplitter::new(
        template_result
            .tool_format
            .unwrap_or(NativeToolFormat::Generic),
    );
    // Accumulate thinking across the entire generation so it can be attached to the final
    // tool-call message (mirroring the OpenAI streaming path). Streaming chunks are still sent
    // for UI display.
    let mut output_filter = ThinkingOutputFilter::new(
        ctx.settings.enable_thinking,
        &template_result.generation_prompt,
    );

    let output_token_count = generation_loop(
        &ctx.loaded.model,
        &mut llama_ctx,
        ctx.settings,
        prompt_token_count,
        effective_ctx,
        |piece| {
            generated_text.push_str(piece);

            let filtered = output_filter.push_text(piece);
            if let Some(thinking) = output_filter.take_streamed_thinking() {
                let mut msg = Message::assistant().with_thinking(thinking, "");
                msg.id = Some(message_id.to_string());
                if tx.blocking_send(Ok((Some(msg), None))).is_err() {
                    return Ok(TokenAction::Stop);
                }
            }

            let text_outside_tool_calls = splitter.push(&filtered.content);
            let (content, stop_seen) = stop_trimmer.push(&text_outside_tool_calls);
            if !content.is_empty() {
                let mut msg = Message::assistant().with_text(content);
                msg.id = Some(message_id.to_string());
                if tx.blocking_send(Ok((Some(msg), None))).is_err() {
                    return Ok(TokenAction::Stop);
                }
            }

            let should_stop = stop_seen
                || template_result
                    .additional_stops
                    .iter()
                    .any(|stop| generated_text.ends_with(stop));
            if should_stop {
                stop_string_emitted = true;
                Ok(TokenAction::Stop)
            } else {
                Ok(TokenAction::Continue)
            }
        },
    )?;

    let filtered = output_filter.finish();
    if !filtered.thinking.is_empty() {
        let mut msg = Message::assistant().with_thinking(&filtered.thinking, "");
        msg.id = Some(message_id.to_string());
        let _ = tx.blocking_send(Ok((Some(msg), None)));
    }

    let mut trailing_text = splitter.push(&filtered.content);
    let stream_end = splitter.finish();
    trailing_text.push_str(&stream_end.content);
    let content = if stop_string_emitted {
        String::new()
    } else {
        let (mut content, stop_seen) = stop_trimmer.push(&trailing_text);
        if !stop_seen {
            content.push_str(&stop_trimmer.finish());
        }
        content
    };
    if !content.is_empty() {
        let mut msg = Message::assistant().with_text(content);
        msg.id = Some(message_id.to_string());
        let _ = tx.blocking_send(Ok((Some(msg), None)));
    }

    if let Some(tool_text) = stream_end.tool_text {
        // Build a single message combining thinking + all tool calls, mirroring the structure
        // produced by the OpenAI streaming path. The agent relies on this combined message to:
        //   1. Extract thinking and attach it to per-tool-request messages
        //   2. Enable merge_split_tool_call_messages to reconstruct the standard
        //      OpenAI format (one assistant msg with N tool_calls, then N tool results)
        let tool_requests: Vec<MessageContent> =
            message_from_native_tool_text(&tool_text, message_id)?
                .map(|message| {
                    message
                        .content
                        .into_iter()
                        .filter(|content| matches!(content, MessageContent::ToolRequest(_)))
                        .collect()
                })
                .unwrap_or_default();

        if tool_requests.is_empty() {
            let mut msg = Message::assistant().with_text(tool_text);
            msg.id = Some(message_id.to_string());
            let _ = tx.blocking_send(Ok((Some(msg), None)));
        } else {
            let mut contents: Vec<MessageContent> = Vec::new();
            if !output_filter.accumulated_thinking().is_empty() {
                contents.push(MessageContent::thinking(
                    output_filter.accumulated_thinking(),
                    "",
                ));
            }
            contents.extend(tool_requests);
            let mut msg = Message::new(
                rmcp::model::Role::Assistant,
                chrono::Utc::now().timestamp(),
                contents,
            );
            msg.id = Some(message_id.to_string());
            let _ = tx.blocking_send(Ok((Some(msg), None)));
        }
    }

    let provider_usage = finalize_usage(
        ctx.log,
        std::mem::take(&mut ctx.model_name),
        "native",
        prompt_token_count,
        output_token_count,
        Some(("generated_text", &generated_text)),
    );
    let _ = ctx.tx.blocking_send(Ok((None, Some(provider_usage))));
    Ok(())
}
