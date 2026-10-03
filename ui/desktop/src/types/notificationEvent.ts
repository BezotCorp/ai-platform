import type { MessageEvent } from './messageEvent';

export type NotificationEvent = Extract<MessageEvent, { type: 'Notification' }>;
