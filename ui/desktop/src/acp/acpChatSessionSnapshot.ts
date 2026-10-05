import type { TokenState } from '../types/tokenState';
import type { ChatState } from '../types/chatState';
import type { Message } from '../types/message';
import type { NotificationEvent } from '../types/notificationEvent';
import { Session } from '../types/session';

export interface AcpChatSessionSnapshot {
  session: Session | undefined;
  messages: Message[];
  tokenState: TokenState;
  notifications: NotificationEvent[];
  progressMessage: string | undefined;
  chatState: ChatState;
  sessionLoadError: string | undefined;
  activePromptAttemptId: string | null;
  activeRunId: string | null;
  pendingCancelPromptAttemptId: string | null;
}
