import type { Message } from '../../types/message';
import type { ToolCallState } from './toolCallState';

export interface AdapterState {
  messages: Message[];
  localSteerTextByMessageId: Map<string, string>;
  toolCallStatesById: Map<string, ToolCallState>;
}
