import type { AcpChatSessionStore } from './acpChatSessionStore';
import type { AcpChatSessionSnapshot } from './acpChatSessionSnapshot';

import type { AcpChatSessionActions } from './acpChatSessionActions';

export interface AcpChatSessionStoreInternal extends AcpChatSessionStore, AcpChatSessionActions {
  subscribe(sessionId: string, listener: (snapshot: AcpChatSessionSnapshot) => void): () => void;
}
