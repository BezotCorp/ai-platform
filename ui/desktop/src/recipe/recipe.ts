import type { RecipeDto } from '@bezotcorp/bcaip-acp-client';

export type Recipe = RecipeDto & {
  // TODO: Separate these from the raw recipe type
  // Properties added for scheduled execution
  scheduledJobId?: string;
  isScheduledExecution?: boolean;
};
