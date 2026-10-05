import type { LiveVoicePhase } from './liveVoicePhase';

export interface LiveVoiceController {
  activeSessionId: string | null;
  liveVoiceSessionId: string | null;
  phase: LiveVoicePhase;
  muted: boolean;
  start: (sessionId: string, initialCommentary?: string) => Promise<void>;
  stop: () => Promise<void>;
  toggleMute: () => void;
}
