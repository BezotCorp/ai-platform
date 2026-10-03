import type { McpAppResource } from './mcpAppResource';
import type { WindowProps } from './windowProps';

export type GooseApp = McpAppResource &
  WindowProps & {
    mcpServers?: string[];
    prd?: string | null;
    deletable?: boolean;
  };
