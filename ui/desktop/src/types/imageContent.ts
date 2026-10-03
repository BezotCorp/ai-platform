import type { Annotations, JsonObject } from '.';

export type ImageContent = {
  _meta?: JsonObject;
  annotations?: Annotations | JsonObject;
  data: string;
  mimeType: string;
};
