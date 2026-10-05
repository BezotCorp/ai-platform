import type {
  GooseSessionNotificationUnstable,
  ProviderDeviceCodeNotificationUnstable,
} from '@aaif/goose-acp-client';
import type { SessionNotification } from '@agentclientprotocol/sdk';
import { AppEvents } from '../constants/appEvents';
import { maybeHandlePlatformEvent } from '../utils/platformEvents';
import { toolNotificationEvent } from './adapter/toolNotifications';
import { acpChatSessionActions, acpChatSessionStore } from './chatSessionStore';
import { publishLiveVoiceInteractionEnded } from './liveVoiceNotifications';

export function handleAcpSessionNotification(notification: SessionNotification): Promise<void> {
  const sessionNameBeforeNotification = acpChatSessionStore.getSnapshot(notification.sessionId)
    ?.session?.name;
  const updatedName =
    notification.update.sessionUpdate === 'session_info_update'
      ? notification.update.title
      : undefined;
  acpChatSessionActions.applyAcpSessionNotification(notification);
  maybeHandleLivePlatformEvent(notification);

  if (updatedName && updatedName !== sessionNameBeforeNotification) {
    window.dispatchEvent(
      new CustomEvent(AppEvents.SESSION_RENAMED, {
        detail: { sessionId: notification.sessionId, newName: updatedName },
      })
    );
  }

  return Promise.resolve();
}

function maybeHandleLivePlatformEvent(notification: SessionNotification): void {
  const update = notification.update;
  if (
    update.sessionUpdate !== 'tool_call_update' ||
    update.status === 'completed' ||
    update.status === 'failed'
  ) {
    return;
  }

  const event = toolNotificationEvent(update);
  if (event?.message.method === 'platform_event') {
    maybeHandlePlatformEvent(event.message, notification.sessionId);
  }
}

export function handleAcpGooseSessionNotification(
  notification: GooseSessionNotificationUnstable
): Promise<void> {
  if (notification.update.sessionUpdate === 'live_voice_interaction_ended') {
    publishLiveVoiceInteractionEnded({
      sessionId: notification.sessionId,
      update: notification.update,
    });
    return Promise.resolve();
  }

  acpChatSessionActions.applyAcpGooseSessionNotification(notification);
  return Promise.resolve();
}

export function handleAcpProviderDeviceCodeNotification(
  notification: ProviderDeviceCodeNotificationUnstable
): Promise<void> {
  window.dispatchEvent(new CustomEvent('goose:device-code', { detail: notification }));
  return Promise.resolve();
}
