import type {
  ToolCall,
  ToolCallUpdate,
} from '@agentclientprotocol/sdk';

import type { Message } from '../../types/message';



import type { AcpChatStateChange } from './acpChatStateChange';
import type { AdapterState } from './adapterState';
import type { BcaipMessageMeta } from './bcaipMessageMeta';
import type { ToolIdentity } from './toolIdentity';
export const DEFAULT_VISIBLE_MESSAGE_METADATA: Message['metadata'] = {
  userVisible: true,
  agentVisible: true,
};

export function messagesChange(state: AdapterState): AcpChatStateChange[] {
  // Pass the live array by reference: the store is the only consumer and it
  // clones on write (applyChatStateChanges). Cloning here as well made every
  // streamed chunk O(messages) twice, which turns session-load replay into
  // O(n^2) on large sessions.
  return [{ type: 'messages', messages: state.messages }];
}

export function cloneMessage(message: Message): Message {
  return {
    ...message,
    content: message.content.map((content) => ({ ...content })),
    metadata: { ...message.metadata },
  };
}

export function getBcaipMessageMeta(update: { _meta?: unknown }): BcaipMessageMeta {
  if (!isRecord(update._meta)) {
    return {};
  }

  const bcaip = update._meta.bcaip;
  if (!isRecord(bcaip)) {
    return {};
  }

  const outputTokenLimitReached = bcaip.outputTokenLimitReached === true;

  return {
    created: typeof bcaip.created === 'number' ? bcaip.created : undefined,
    messageId: typeof bcaip.messageId === 'string' ? bcaip.messageId : undefined,
    outputTokenLimitReached: outputTokenLimitReached ? true : undefined,
    fallbackContent: bcaip.fallbackContent === true ? true : undefined,
    steer: bcaip.steer === true ? true : undefined,
  };
}

export function getBcaipActiveRunId(update: { _meta?: unknown }): string | null | undefined {
  if (!isRecord(update._meta)) {
    return undefined;
  }

  const bcaip = update._meta.bcaip;
  if (!isRecord(bcaip) || !('activeRunId' in bcaip)) {
    return undefined;
  }

  return typeof bcaip.activeRunId === 'string' || bcaip.activeRunId === null
    ? bcaip.activeRunId
    : undefined;
}

export function getBcaipQueuedSteer(update: { _meta?: unknown }): string | undefined {
  if (!isRecord(update._meta)) return undefined;
  const bcaip = update._meta.bcaip;
  if (!isRecord(bcaip) || !isRecord(bcaip.queuedSteer)) return undefined;
  return typeof bcaip.queuedSteer.messageId === 'string' ? bcaip.queuedSteer.messageId : undefined;
}

export function rawInputToArguments(rawInput: unknown): Record<string, unknown> {
  return isRecord(rawInput) ? rawInput : {};
}

export function toolIdentity(update: ToolCall | ToolCallUpdate): ToolIdentity {
  if (!isRecord(update._meta)) {
    return {};
  }

  const bcaip = update._meta.bcaip;
  if (!isRecord(bcaip) || !isRecord(bcaip.toolCall)) {
    return {};
  }

  return {
    toolName: typeof bcaip.toolCall.toolName === 'string' ? bcaip.toolCall.toolName : undefined,
    extensionName:
      typeof bcaip.toolCall.extensionName === 'string' ? bcaip.toolCall.extensionName : undefined,
  };
}

export function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null;
}

export type { AcpChatStateChange } from './acpChatStateChange';
export type { AdapterState } from './adapterState';
export type { ToolCallState } from './toolCallState';
export type { BcaipMessageMeta } from './bcaipMessageMeta';
export type { ToolIdentity } from './toolIdentity';
