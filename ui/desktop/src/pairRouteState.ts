import type { UserInput } from './types/message';

export interface PairRouteState {
  resumeSessionId?: string;
  initialMessage?: UserInput;
  noAutoSubmit?: boolean;
}
