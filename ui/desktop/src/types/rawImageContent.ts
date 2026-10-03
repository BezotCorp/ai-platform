import type { ContentAnnotations } from './contentAnnotations';
import type { JsonObject } from './jsonObject';

export type RawImageContent = {
  _meta?: JsonObject;
  annotations?: ContentAnnotations;
  data: string;
  mimeType: string;
};
