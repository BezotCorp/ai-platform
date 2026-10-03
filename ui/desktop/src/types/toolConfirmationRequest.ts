import type { JsonObject } from './jsonObject';

export type ToolConfirmationRequest = {
  arguments: JsonObject;
  id: string;
  prompt?: string | null;
  toolName: string;
};
