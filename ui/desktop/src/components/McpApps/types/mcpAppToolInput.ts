import type { McpUiToolInputNotification } from '@modelcontextprotocol/ext-apps/app-bridge';

/**
 * Tool input from the message stream.
 * McpAppRenderer extracts `.arguments` when passing to the SDK's AppRenderer.
 */
export type McpAppToolInput = McpUiToolInputNotification['params'];
