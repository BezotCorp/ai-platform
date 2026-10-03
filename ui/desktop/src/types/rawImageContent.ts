import type { ContentAnnotations, JsonObject } from '.';

export type RawImageContent = {
  _meta?: JsonObject;
  annotations?: ContentAnnotations;
  data: string;
  mimeType: string;
};
