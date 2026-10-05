import type { ChatState } from '../types/chatState';
import type { AcpSessionNotificationAdapter } from './sessionNotificationAdapter';
import type { AcpChatSessionSnapshot } from './acpChatSessionSnapshot';

export interface StoreEntry extends AcpChatSessionSnapshot {
  adapter: AcpSessionNotificationAdapter;
  promptCancellationRestoreState: {
    activeRunId: string | null;
    chatState: ChatState;
    pendingUserInputRequestIds: Set<string>;
  } | null;
  pendingUserInputRequestIds: Set<string>;
  pendingLocalSteerMessageIds: Set<string>;
  preConfirmedSteerMessageIds: Set<string>;
  // Cached result of the last notify(); reused while a session-load replay is
  // in flight so per-notification reads don't deep-clone the growing message
  // array (see applyAcpSessionNotification / getSnapshot).
  lastSnapshot?: AcpChatSessionSnapshot;
}
