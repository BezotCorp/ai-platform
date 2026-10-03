export type ThinkingContent = {
  signature: string;
  thinking: string;
};

export function getThinkingContent(message: { content: Array<{ type: string }> }): string | null {
  const parts: string[] = [];

  for (const content of message.content) {
    if (content.type === 'thinking' && 'thinking' in content && content.thinking) {
      parts.push(String(content.thinking));
    }
  }

  return parts.length > 0 ? parts.join('') : null;
}
