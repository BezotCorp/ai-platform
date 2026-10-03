import type { ContentIcon } from './contentIcon';
import type { JsonObject } from './jsonObject';

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
