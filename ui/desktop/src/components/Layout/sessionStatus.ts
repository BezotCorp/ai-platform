import { StreamState } from './streamState';

export interface SessionStatus {
  streamState: StreamState;
  hasUnreadActivity: boolean;
}
