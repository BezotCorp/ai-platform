import type { BcaipSessionNotificationUnstable } from '@bezotcorp/bcaip-acp-client';

export type LiveVoiceInteractionEndedNotification = {
  sessionId: string;
  update: Extract<
    BcaipSessionNotificationUnstable['update'],
    { sessionUpdate: 'live_voice_interaction_ended' }
  >;
};
