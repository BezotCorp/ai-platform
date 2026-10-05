import type { Annotations } from './annotations';
import type { JsonObject } from './jsonObject';

export type ImageContent = {
  _meta?: JsonObject;
  annotations?: Annotations | JsonObject;
  data: string;
  mimeType: string;
};
