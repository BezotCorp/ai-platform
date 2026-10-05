import type { GooseSessionNotificationUnstable } from '@aaif/goose-acp-client';

export type LiveVoiceInteractionEndedNotification = {
  sessionId: string;
  update: Extract<
    GooseSessionNotificationUnstable['update'],
    { sessionUpdate: 'live_voice_interaction_ended' }
  >;
};
