import type { ToolRequestMessageContent } from '../../types/toolRequestMessageContent';
import type { ToolResponseMessageContent } from '../../types/toolResponseMessageContent';
import type { NotificationEvent } from '../../types/notificationEvent';
import type { ToolConfirmationData } from '../../types/toolConfirmationData';

export interface ToolCallWithResponseProps {
  sessionId?: string;
  isCancelledMessage: boolean;
  toolRequest: ToolRequestMessageContent;
  toolResponse?: ToolResponseMessageContent;
  notifications?: NotificationEvent[];
  isStreamingMessage?: boolean;
  isPendingApproval: boolean;
  append?: (value: string) => void;
  confirmationContent?: ToolConfirmationData;
  isApprovalClicked?: boolean;
}
