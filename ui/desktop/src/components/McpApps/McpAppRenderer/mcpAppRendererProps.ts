import type { CallToolResult } from '@modelcontextprotocol/sdk/types.js';
import { GooseDisplayMode, McpAppToolCancelled, McpAppToolInput, McpAppToolInputPartial, OnDisplayModeChange } from '../types';

export interface McpAppRendererProps {
  resourceUri: string;
  extensionName: string;
  toolName?: string;
  sessionId?: string | null;
  toolInput?: McpAppToolInput;
  toolInputPartial?: McpAppToolInputPartial;
  toolResult?: CallToolResult;
  toolCancelled?: McpAppToolCancelled;
  append?: (text: string) => void;
  displayMode?: GooseDisplayMode;
  cachedHtml?: string;
  onDisplayModeChange?: OnDisplayModeChange;
}
