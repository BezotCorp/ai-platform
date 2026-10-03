import type { JsonObject } from '.';

export type ContentIcon = {
  mimeType?: string;
  sizes?: string[];
  src: string;
  theme?: 'light' | 'dark' | JsonObject;
};
