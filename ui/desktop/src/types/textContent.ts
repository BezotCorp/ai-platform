import type { Annotations, JsonObject } from '.';

export type TextContent = {
  _meta?: JsonObject;
  annotations?: Annotations | JsonObject;
  text: string;
};
