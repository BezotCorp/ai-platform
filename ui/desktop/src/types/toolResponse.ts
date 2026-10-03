import type { JsonObject } from '.';

export type ToolResponse = {
  id: string;
  metadata?: JsonObject;
  toolResult: JsonObject;
};
