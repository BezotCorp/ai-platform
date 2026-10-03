import type { UserInput } from './types/userInput';

export interface PairRouteState {
  resumeSessionId?: string;
  initialMessage?: UserInput;
  noAutoSubmit?: boolean;
}
