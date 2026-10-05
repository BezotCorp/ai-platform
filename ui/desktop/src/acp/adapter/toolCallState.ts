import type { ToolCallUpdate } from '@agentclientprotocol/sdk';

export type ToolCallState = Omit<ToolCallUpdate, '_meta'>;
