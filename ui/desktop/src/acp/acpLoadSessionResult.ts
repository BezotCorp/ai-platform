import type { LoadSessionResponse, SessionInfo } from '@agentclientprotocol/sdk';
import type { LoadSessionMeta } from './loadSessionMeta';

/**
 * Complete result of loading a session through ACP.
 */
export interface AcpLoadSessionResult {
  sessionInfo: SessionInfo;
  response: LoadSessionResponse;
  meta: LoadSessionMeta;
}
