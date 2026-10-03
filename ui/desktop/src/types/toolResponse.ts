import type { JsonObject } from './jsonObject';

export type ToolResponse = {
  id: string;
  metadata?: JsonObject;
  toolResult: JsonObject;
};
