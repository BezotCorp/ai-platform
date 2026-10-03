import type { ToolConfirmationRequest } from './toolConfirmationRequest';

export type ToolConfirmationRequestContent = ToolConfirmationRequest & {
  type: 'toolConfirmationRequest';
};
