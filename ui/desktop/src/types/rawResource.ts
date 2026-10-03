import type { ContentIcon, JsonObject } from '.';

export type RawResource = {
  _meta?: JsonObject;
  description?: string;
  icons?: ContentIcon[];
  mimeType?: string;
  name: string;
  size?: number;
  title?: string;
  uri: string;
};
