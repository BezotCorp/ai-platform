import type { McpUiStyleVariableKey } from '@modelcontextprotocol/ext-apps/app-bridge';

export type BaseTokenKey = Extract<
  McpUiStyleVariableKey,
  `--font-${string}` | `--border-radius-${string}` | `--border-width-${string}`
>;
