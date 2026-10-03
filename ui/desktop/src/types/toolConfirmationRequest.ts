import type { JsonObject } from '.';

export type ToolConfirmationRequest = {
  arguments: JsonObject;
  id: string;
  prompt?: string | null;
  toolName: string;
};
