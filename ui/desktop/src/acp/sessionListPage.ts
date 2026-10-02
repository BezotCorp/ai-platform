import type { SessionListItem } from './sessionListItem';

/**
 * Paginated session-list response exposed by the ACP adapter.
 */
export interface SessionListPage {
  sessions: SessionListItem[];
  nextCursor: string | null;
}
