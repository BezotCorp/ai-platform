import type { UserInput } from '../../../types/userInput';
import type { LiveVoiceController } from '../../../liveVoice/useLiveVoice';

export interface AppLayoutProps {
  activeSessions: Array<{
    sessionId: string;
    initialMessage?: UserInput;
    noAutoSubmit?: boolean;
  }>;
  liveVoice: LiveVoiceController;
}
