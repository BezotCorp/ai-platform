import type { SessionType } from '../types/sessionType';

/**
 * Serialized data used to construct a `SessionListItem`.
 *
 * ACP transports dates as strings. The application converts them to `AppDate`
 * as soon as they enter the session-list domain.
 */
export interface SessionListItemData {
  id: string;
  name: string;
  workingDir: string;
  updatedAt: string;
  messageCount: number;
  lastMessageAt?: string;
  createdAt: string;
  archivedAt?: string;
  projectId?: string;
  providerId?: string;
  modelId?: string;
  userSetName?: boolean;
  hasRecipe?: boolean;
  sessionType?: SessionType;
}
