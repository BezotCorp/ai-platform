import type { JsonObject } from '.';

export type ToolRequest = {
  _meta?: JsonObject;
  id: string;
  metadata?: JsonObject;
  toolCall: JsonObject;
};
