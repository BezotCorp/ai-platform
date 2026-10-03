import type { ImageData, MessageContent, MessageMetadata, Role } from '.';

export type Message = {
  content: MessageContent[];
  created: number;
  id?: string | null;
  metadata: MessageMetadata;
  role: Role;
};

export function createUserMessage(text: string, images?: ImageData[]): Message {
  const content: Message['content'] = [];

  if (text.trim()) {
    content.push({ type: 'text', text });
  }

  if (images && images.length > 0) {
    images.forEach((img) => {
      content.push({
        type: 'image',
        data: img.data,
        mimeType: img.mimeType,
        ...(img._meta ? { _meta: img._meta } : {}),
        ...(img.annotations ? { annotations: img.annotations } : {}),
      });
    });
  }

  return {
    id: generateMessageId(),
    role: 'user',
    created: Math.floor(Date.now() / 1000),
    content,
    metadata: { userVisible: true, agentVisible: true },
  };
}

export function generateMessageId(): string {
  return Math.random().toString(36).substring(2, 10);
}
