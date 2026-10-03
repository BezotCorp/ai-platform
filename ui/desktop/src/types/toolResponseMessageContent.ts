import type { Message, ToolResponse } from '.';

export type ToolResponseMessageContent = ToolResponse & { type: 'toolResponse' };

export function getToolResponses(message: Message): ToolResponseMessageContent[] {
  return message.content.filter(
    (content): content is ToolResponseMessageContent => content.type === 'toolResponse'
  );
}
