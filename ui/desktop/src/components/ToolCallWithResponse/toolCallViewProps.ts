import type { ToolResponseMessageContent } from '../../types/toolResponseMessageContent';
import type { NotificationEvent } from '../../types/notificationEvent';

export interface ToolCallViewProps {
  isCancelledMessage: boolean;
  toolCall: {
    name: string;
    arguments: Record<string, unknown>;
  };
  toolResponse?: ToolResponseMessageContent;
  notifications?: NotificationEvent[];
  isStreamingMessage?: boolean;
}
