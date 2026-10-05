import type { ChatState } from '../../../types/chatState';
import type { TokenState } from '../../../types/tokenState';
import type { Message } from '../../../types/message';
import type { ImageData } from '../../../types/imageData';
import type { NotificationEvent } from '../../../types/notificationEvent';
import type { UserInput } from '../../../types/userInput';
import type { Session } from '../../../types/session';

export interface UseChatSessionResult {
  session?: Session;
  messages: Message[];
  chatState: ChatState;
  progressMessage?: string;
  updateSession: (updater: (session: Session) => Session) => void;
  handleSubmit: (input: UserInput) => Promise<void>;
  onSteerQueuedMessage?: (input: UserInput) => Promise<boolean>;
  submitElicitationResponse: (
    elicitationId: string,
    userData: Record<string, unknown>
  ) => Promise<boolean>;
  stopStreaming: () => void;
  retrySessionLoad: () => Promise<void>;
  sessionLoadError?: string;
  tokenState: TokenState;
  notifications: Map<string, NotificationEvent[]>;
  pauseQueueOnStop: boolean;
  queueProcessingBlocked: boolean;
  hasActiveRun: boolean;
  onMessageUpdate: (
    messageId: string,
    newContent: string,
    editType: 'fork' | 'edit',
    retainedImages: ImageData[]
  ) => Promise<void>;
}
