import type { AppDate } from '../utils/appDate';
import type { SessionData } from '.';

/**
 * Changes that can be applied to an application `Session`.
 *
 * Serialized session data uses strings for dates, while the application
 * domain uses `AppDate`.
 */
export type SessionChanges = Omit<
  Partial<SessionData>,
  'created_at' | 'updated_at' | 'last_message_at' | 'archived_at'
> & {
  created_at?: AppDate;
  updated_at?: AppDate;
  last_message_at?: AppDate | null;
  archived_at?: AppDate | null;
};
