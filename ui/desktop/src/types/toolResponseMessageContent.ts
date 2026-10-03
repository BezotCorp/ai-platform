import type { Message } from './message';
import type { ToolResponse } from './toolResponse';

export type ToolResponseMessageContent = ToolResponse & { type: 'toolResponse' };

export function getToolResponses(message: Message): ToolResponseMessageContent[] {
  return message.content.filter(
    (content): content is ToolResponseMessageContent => content.type === 'toolResponse'
  );
}
