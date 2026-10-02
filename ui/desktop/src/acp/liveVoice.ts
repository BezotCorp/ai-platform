import type {
  LiveVoiceAvailabilityResponseUnstable,
  LiveVoiceStartResponseUnstable,
} from '@aaif/goose-acp-client';
import { getAcpClient } from './acpConnection';

export async function acpGetLiveVoiceAvailability(
  sessionId?: string
): Promise<LiveVoiceAvailabilityResponseUnstable> {
  const { goose } = await getAcpClient();
  const useLegacyAgentLoop = await window.electron.getSetting('useLegacyAgentLoop');
  return goose.sessionLiveVoiceAvailabilityUnstable({
    ...(sessionId ? { sessionId } : {}),
    _meta: { goose: { unrolledAgentLoop: !useLegacyAgentLoop } },
  });
}

export async function acpStartLiveVoice(
  sessionId: string,
  offerSdp: string
): Promise<LiveVoiceStartResponseUnstable> {
  const { goose } = await getAcpClient();
  const useLegacyAgentLoop = await window.electron.getSetting('useLegacyAgentLoop');
  return goose.sessionLiveVoiceStartUnstable({
    sessionId,
    offerSdp,
    _meta: { goose: { unrolledAgentLoop: !useLegacyAgentLoop } },
  });
}

export async function acpStopLiveVoice(sessionId: string, interactionId: string): Promise<void> {
  const { goose } = await getAcpClient();
  await goose.sessionLiveVoiceStopUnstable({ sessionId, interactionId });
}
