import type { Annotations } from './annotations';
import type { JsonObject } from './jsonObject';

export type TextContent = {
  _meta?: JsonObject;
  annotations?: Annotations | JsonObject;
  text: string;
};
