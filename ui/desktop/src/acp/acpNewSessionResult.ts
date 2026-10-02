import type { SessionInfo } from '@agentclientprotocol/sdk';
import type { LoadSessionMeta } from './loadSessionMeta';

/**
 * Result returned after creating a new ACP session.
 */
export interface AcpNewSessionResult {
  sessionId: string;
  sessionInfo: SessionInfo;
  meta: LoadSessionMeta;
}
