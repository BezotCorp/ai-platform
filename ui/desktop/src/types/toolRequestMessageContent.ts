import type { Message, ToolRequest } from '.';

export type ToolRequestMessageContent = ToolRequest & { type: 'toolRequest' };

export function getToolRequests(message: Message): ToolRequestMessageContent[] {
  return message.content.filter(
    (content): content is ToolRequestMessageContent => content.type === 'toolRequest'
  );
}
