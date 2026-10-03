import type { McpAppResource, WindowProps } from '.';

export type GooseApp = McpAppResource &
  WindowProps & {
    mcpServers?: string[];
    prd?: string | null;
    deletable?: boolean;
  };
