import type { SessionType } from '../types/sessionType';

/**
 * BCAIP-specific metadata carried through ACP `SessionInfo._meta`.
 */
export interface GooseSessionInfoMeta {
  messageCount?: number;
  createdAt?: string;
  lastMessageAt?: string;
  archivedAt?: string;
  projectId?: string;
  providerId?: string;
  modelId?: string;
  sessionType?: SessionType;
  userSetName?: boolean;
  hasRecipe?: boolean;
  lastMessageSnippet?: string;
}
