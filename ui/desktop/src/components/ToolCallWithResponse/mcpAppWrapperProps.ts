import type { ToolRequestMessageContent } from '../../types/toolRequestMessageContent';
import type { ToolResponseMessageContent } from '../../types/toolResponseMessageContent';

export interface McpAppWrapperProps {
  toolRequest: ToolRequestMessageContent;
  toolResponse?: ToolResponseMessageContent;
  sessionId: string;
  append?: (value: string) => void;
}
