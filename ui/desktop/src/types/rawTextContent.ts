import type { ContentAnnotations } from './contentAnnotations';
import type { JsonObject } from './jsonObject';

export type RawTextContent = {
  _meta?: JsonObject;
  annotations?: ContentAnnotations;
  text: string;
};
