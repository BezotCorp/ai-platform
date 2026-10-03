import type { MessageContent } from '.';

export type ImageMessageContent = Extract<MessageContent, { type: 'image' }>;
