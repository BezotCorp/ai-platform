import type {
  ActionRequired,
  ImageContent,
  RedactedThinkingContent,
  SystemNotificationContent,
  TextContent,
  ThinkingContent,
  ToolConfirmationRequest,
  ToolRequest,
  ToolResponse,
} from '.';

export type MessageContent =
  | (TextContent & { type: 'text' })
  | (ImageContent & { type: 'image' })
  | (ToolRequest & { type: 'toolRequest' })
  | (ToolResponse & { type: 'toolResponse' })
  | (ToolConfirmationRequest & { type: 'toolConfirmationRequest' })
  | (ActionRequired & { type: 'actionRequired' })
  | (ThinkingContent & { type: 'thinking' })
  | (RedactedThinkingContent & { type: 'redactedThinking' })
  | (SystemNotificationContent & { type: 'systemNotification' });

export function getTextAndImageContent(message: {
  content: MessageContent[];
  role: 'user' | 'assistant';
}): {
  textContent: string;
  imagePaths: string[];
} {
  let textContent = '';
  const imagePaths: string[] = [];

  for (const content of message.content) {
    if (content.type === 'text') {
      textContent += content.text;
    } else if (content.type === 'image') {
      imagePaths.push(`data:${content.mimeType};base64,${content.data}`);
    }
  }

  if (message.role === 'assistant') {
    textContent = stripToolCallMarkers(textContent);
  }

  return { textContent, imagePaths };
}

function stripToolCallMarkers(text: string): string {
  return text
    .replace(/<\|tool_calls_section_begin\|>[\s\S]*?<\|tool_calls_section_end\|>/g, '')
    .replace(/<\|tool_call_begin\|>[\s\S]*?<\|tool_call_end\|>/g, '')
    .replace(/<\|tool_call_argument_begin\|>[\s\S]*?<\|tool_call_argument_end\|>/g, '')
    .trim();
}
