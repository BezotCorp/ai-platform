import type { InferenceMetadata } from './inferenceMetadata';
import type { MessageUsage } from './messageUsage';

export type MessageMetadata = {
  agentVisible: boolean;
  fallbackContent?: boolean;
  inference?: InferenceMetadata | null;
  outputTokenLimitReached?: boolean;
  steer?: boolean;
  usage?: MessageUsage | null;
  userVisible: boolean;
};
