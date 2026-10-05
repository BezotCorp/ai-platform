import type {
  ToolCall,
  ToolCallUpdate,
} from '@agentclientprotocol/sdk';

import type { Message } from '../../types/message';



import type { AcpChatStateChange } from './acpChatStateChange';
import type { AdapterState } from './adapterState';
import type { GooseMessageMeta } from './gooseMessageMeta';
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

export function getGooseMessageMeta(update: { _meta?: unknown }): GooseMessageMeta {
  if (!isRecord(update._meta)) {
    return {};
  }

  const goose = update._meta.goose;
  if (!isRecord(goose)) {
    return {};
  }

  const outputTokenLimitReached = goose.outputTokenLimitReached === true;

  return {
    created: typeof goose.created === 'number' ? goose.created : undefined,
    messageId: typeof goose.messageId === 'string' ? goose.messageId : undefined,
    outputTokenLimitReached: outputTokenLimitReached ? true : undefined,
    fallbackContent: goose.fallbackContent === true ? true : undefined,
    steer: goose.steer === true ? true : undefined,
  };
}

export function getGooseActiveRunId(update: { _meta?: unknown }): string | null | undefined {
  if (!isRecord(update._meta)) {
    return undefined;
  }

  const goose = update._meta.goose;
  if (!isRecord(goose) || !('activeRunId' in goose)) {
    return undefined;
  }

  return typeof goose.activeRunId === 'string' || goose.activeRunId === null
    ? goose.activeRunId
    : undefined;
}

export function getGooseQueuedSteer(update: { _meta?: unknown }): string | undefined {
  if (!isRecord(update._meta)) return undefined;
  const goose = update._meta.goose;
  if (!isRecord(goose) || !isRecord(goose.queuedSteer)) return undefined;
  return typeof goose.queuedSteer.messageId === 'string' ? goose.queuedSteer.messageId : undefined;
}

export function rawInputToArguments(rawInput: unknown): Record<string, unknown> {
  return isRecord(rawInput) ? rawInput : {};
}

export function toolIdentity(update: ToolCall | ToolCallUpdate): ToolIdentity {
  if (!isRecord(update._meta)) {
    return {};
  }

  const goose = update._meta.goose;
  if (!isRecord(goose) || !isRecord(goose.toolCall)) {
    return {};
  }

  return {
    toolName: typeof goose.toolCall.toolName === 'string' ? goose.toolCall.toolName : undefined,
    extensionName:
      typeof goose.toolCall.extensionName === 'string' ? goose.toolCall.extensionName : undefined,
  };
}

export function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null;
}

export type { AcpChatStateChange } from './acpChatStateChange';
export type { AdapterState } from './adapterState';
export type { ToolCallState } from './toolCallState';
export type { GooseMessageMeta } from './gooseMessageMeta';
export type { ToolIdentity } from './toolIdentity';
