import type { ChatState } from '../../types/chatState';
import type { Message } from '../../types/message';
import type { UserInput } from '../../types/userInput';
import type { Session } from '../../types/session';

/**
 * Auto-submit scenarios:
 * 1. New session with initial message from Hub (message_count === 0, has initialMessage)
 * 2. Forked session with edited message (shouldStartAgent + initialMessage)
 * 3. Resume with shouldStartAgent (continue existing conversation)
 */

export interface UseAutoSubmitProps {
  sessionId: string;
  session: Session | undefined;
  messages: Message[];
  chatState: ChatState;
  initialMessage: UserInput | undefined;
  canAutoSubmit?: boolean;
  handleSubmit: (input: UserInput) => void;
}
