import type { McpUiStyleVariableKey } from '@modelcontextprotocol/ext-apps/app-bridge';
import type { BaseTokenKey } from './baseTokenKey';

export type ColorTokenKey = Exclude<McpUiStyleVariableKey, BaseTokenKey>;
