import type { Recipe } from '../recipe';
import type { UserInput } from '../types/userInput';
import type { View } from './view';

export type ViewOptions = {
  showEnvVars?: boolean;
  deepLinkConfig?: unknown;
  error?: string;
  recipe?: Recipe;
  parentView?: View;
  parentViewOptions?: ViewOptions;
  disableAnimation?: boolean;
  initialMessage?: UserInput;
  resumeSessionId?: string;
  startLiveVoice?: boolean;
  pendingScheduleDeepLink?: string;
};
