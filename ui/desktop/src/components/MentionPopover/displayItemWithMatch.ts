import type { DisplayItem } from './displayItem';

export interface DisplayItemWithMatch extends DisplayItem {
  matchScore: number;
  matches: number[];
  matchedText: string;
}
