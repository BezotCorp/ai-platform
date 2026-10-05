import { ChatType } from '../types/chatType';
import type { UserInput } from '../types/userInput';
import type { LiveVoiceController } from '../liveVoice/useLiveVoice';

export interface ChatSessionsContainerProps {
  setChat: (chat: ChatType) => void;
  activeSessions: Array<{
    sessionId: string;
    initialMessage?: UserInput;
    noAutoSubmit?: boolean;
  }>;
  liveVoice: LiveVoiceController;
}
