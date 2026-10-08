import type { LiveVoiceAvailabilityResponseUnstable } from '@bezotcorp/bcaip-acp-client';
import type { LiveVoiceController } from '../../liveVoice/useLiveVoice';

export type ChatInputLiveVoice = Pick<
  LiveVoiceController,
  'phase' | 'muted' | 'stop' | 'toggleMute'
> & {
  availability: LiveVoiceAvailabilityResponseUnstable | null;
  activeInAnotherSession: boolean;
  start: () => Promise<void>;
};
