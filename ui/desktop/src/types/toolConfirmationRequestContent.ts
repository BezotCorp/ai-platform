import type { ToolConfirmationRequest } from '.';

export type ToolConfirmationRequestContent = ToolConfirmationRequest & {
  type: 'toolConfirmationRequest';
};
