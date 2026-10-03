import type { JsonObject } from './jsonObject';

export type ContentIcon = {
  mimeType?: string;
  sizes?: string[];
  src: string;
  theme?: 'light' | 'dark' | JsonObject;
};
