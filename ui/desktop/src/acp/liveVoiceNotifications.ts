import type { LiveVoiceInteractionEndedNotification } from './liveVoiceInteractionEndedNotification';
import type { LiveVoiceInteractionEndedListener } from './liveVoiceInteractionEndedListener';
const listeners = new Set<LiveVoiceInteractionEndedListener>();

export function subscribeToLiveVoiceInteractionEnded(
  listener: LiveVoiceInteractionEndedListener
): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function publishLiveVoiceInteractionEnded(
  notification: LiveVoiceInteractionEndedNotification
): void {
  for (const listener of listeners) {
    listener(notification);
  }
}
