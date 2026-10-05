import type { HfModelVariant } from './hfModelVariant';

export type RepoVariantsResponse = {
  variants: HfModelVariant[];
  recommendedIndex: number | null;
  availableMemoryBytes: number;
  downloadedQuants: string[];
  downloadedVariants: string[];
};
