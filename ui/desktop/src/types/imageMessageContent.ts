import type { MessageContent } from './messageContent';

export type ImageMessageContent = Extract<MessageContent, { type: 'image' }>;
