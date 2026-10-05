import type { AcpChatSessionSnapshot } from './acpChatSessionSnapshot';

export interface AcpChatSessionStore {
  getSnapshot(sessionId: string): AcpChatSessionSnapshot | undefined;
}
