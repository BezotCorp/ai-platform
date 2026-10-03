import type { InferenceMetadata, MessageUsage } from '.';

export type MessageMetadata = {
  agentVisible: boolean;
  fallbackContent?: boolean;
  inference?: InferenceMetadata | null;
  outputTokenLimitReached?: boolean;
  steer?: boolean;
  usage?: MessageUsage | null;
  userVisible: boolean;
};
