import type { DisplayItemType } from './displayItemType';

export interface DisplayItem {
  name: string;
  extra: string;
  itemType: DisplayItemType;
  relativePath: string;
  insertText?: string;
}
