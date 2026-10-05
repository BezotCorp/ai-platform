import type { HfModelVariant } from '../../../../acp/hfModelVariant';


export interface RepoData {
  variants: HfModelVariant[];
  recommendedIndex: number | null;
  availableMemoryBytes: number;
  downloadedQuants: Set<string>;
  downloadedVariants: Set<string>;
}
