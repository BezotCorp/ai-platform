import type { Annotations } from './annotation';
import type { JsonObject } from './jsonObject';

export type ImageContent = {
  _meta?: JsonObject;
  annotations?: Annotations | JsonObject;
  data: string;
  mimeType: string;
};
