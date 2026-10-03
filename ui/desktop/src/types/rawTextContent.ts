import type { ContentAnnotations, JsonObject } from '.';

export type RawTextContent = {
  _meta?: JsonObject;
  annotations?: ContentAnnotations;
  text: string;
};
