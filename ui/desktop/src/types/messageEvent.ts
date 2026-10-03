import type { Conversation, JsonObject, Message, MessageUsage, TokenState } from '.';

export type MessageEvent =
  | {
      message: Message;
      token_state: TokenState;
      type: 'Message';
    }
  | {
      message_id?: string | null;
      usage: MessageUsage;
      type: 'MessageUsage';
    }
  | {
      error: string;
      type: 'Error';
    }
  | {
      reason: string;
      token_state: TokenState;
      type: 'Finish';
    }
  | {
      message: JsonObject;
      request_id: string;
      type: 'Notification';
    }
  | {
      conversation: Conversation;
      type: 'UpdateConversation';
    }
  | {
      request_ids: string[];
      type: 'ActiveRequests';
    }
  | {
      type: 'Ping';
    };
