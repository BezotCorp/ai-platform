import type { Message } from '../types/message';
import type { NotificationEvent } from '../types/notificationEvent';
import type { ToolRenderState } from './messageRowContext';

export interface GooseMessageProps {
  sessionId: string;
  message: Message;
  hideTimestamp: boolean;
  toolStates: readonly ToolRenderState[];
  toolNotifications: readonly (NotificationEvent[] | undefined)[];
  toolConfirmationShownInline: boolean;
  append: (value: string) => void;
  isStreaming: boolean;
  submitElicitationResponse?: (
    elicitationId: string,
    userData: Record<string, unknown>
  ) => Promise<boolean>;
}
