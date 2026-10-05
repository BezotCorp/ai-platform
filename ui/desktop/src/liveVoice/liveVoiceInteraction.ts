import type { LiveVoiceInteractionEndedNotification } from '../acp/liveVoiceInteractionEndedNotification';

import type { LiveVoiceMediaSession } from './liveVoiceMediaSession';

export interface LiveVoiceInteraction {
  sessionId: string;
  interactionId?: string;
  remoteStartPending: boolean;
  media: LiveVoiceMediaSession;
  mediaReady: boolean;
  invalidated: boolean;
  acpConnectionLost: boolean;
  pendingOutcomesByInteractionId: Map<
    string,
    LiveVoiceInteractionEndedNotification['update']['outcome']
  >;
}
