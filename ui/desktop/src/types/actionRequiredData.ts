import type { JsonObject } from '.';

export type ActionRequiredData =
  | {
      actionType: 'toolConfirmation';
      arguments: JsonObject;
      generation?: string;
      id: string;
      prompt?: string | null;
      toolName: string;
    }
  | {
      actionType: 'elicitation';
      id: string;
      message: string;
      requested_schema: unknown;
    }
  | {
      action?: string;
      actionType: 'elicitationResponse';
      id: string;
      user_data: unknown;
    };
