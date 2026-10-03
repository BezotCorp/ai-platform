import type { JsonObject } from './jsonObject';

export type ToolRequest = {
  _meta?: JsonObject;
  id: string;
  metadata?: JsonObject;
  toolCall: JsonObject;
};
