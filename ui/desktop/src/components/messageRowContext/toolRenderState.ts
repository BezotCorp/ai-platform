import type { ToolConfirmationData } from '../../types/toolConfirmationData';
import type { ToolResponseMessageContent } from '../../types/toolResponseMessageContent';

export interface ToolRenderState {
  requestId: string;
  response: ToolResponseMessageContent | undefined;
  confirmation: ToolConfirmationData | undefined;
  isPending: boolean;
}
