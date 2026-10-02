import type { AppDate } from '../utils/appDate';
import type { SessionListItemData } from './sessionListItemData';

/**
 * Domain-aware changes that can be applied to a `SessionListItem`.
 */
export type SessionListItemChanges = Omit<
  Partial<SessionListItemData>,
  'updatedAt' | 'lastMessageAt' | 'createdAt' | 'archivedAt'
> & {
  updatedAt?: AppDate;
  lastMessageAt?: AppDate;
  createdAt?: AppDate;
  archivedAt?: AppDate;
};
