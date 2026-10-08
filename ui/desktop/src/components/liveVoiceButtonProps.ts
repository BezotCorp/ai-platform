import type { LiveVoiceAvailabilityResponseUnstable } from '@bezotcorp/bcaip-acp-client';
import { type LiveVoicePhase } from '../liveVoice/useLiveVoice';

export interface LiveVoiceButtonProps {
  availability: LiveVoiceAvailabilityResponseUnstable | null;
  composerEmpty: boolean;
  phase: LiveVoicePhase;
  muted: boolean;
  activeInAnotherSession: boolean;
  onStart: () => void;
  onStop: () => void;
  onToggleMute: () => void;
}
