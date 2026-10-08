import type {
  LiveVoiceAvailabilityResponseUnstable,
  LiveVoiceStartResponseUnstable,
} from '@bezotcorp/bcaip-acp-client';
import { getAcpClient } from './acpConnection';

export async function acpGetLiveVoiceAvailability(
  sessionId?: string
): Promise<LiveVoiceAvailabilityResponseUnstable> {
  const { bcaip } = await getAcpClient();
  const useLegacyAgentLoop = await window.electron.getSetting('useLegacyAgentLoop');
  return bcaip.sessionLiveVoiceAvailabilityUnstable({
    ...(sessionId ? { sessionId } : {}),
    _meta: { bcaip: { unrolledAgentLoop: !useLegacyAgentLoop } },
  });
}

export async function acpStartLiveVoice(
  sessionId: string,
  offerSdp: string
): Promise<LiveVoiceStartResponseUnstable> {
  const { bcaip } = await getAcpClient();
  const useLegacyAgentLoop = await window.electron.getSetting('useLegacyAgentLoop');
  return bcaip.sessionLiveVoiceStartUnstable({
    sessionId,
    offerSdp,
    _meta: { bcaip: { unrolledAgentLoop: !useLegacyAgentLoop } },
  });
}

export async function acpStopLiveVoice(sessionId: string, interactionId: string): Promise<void> {
  const { bcaip } = await getAcpClient();
  await bcaip.sessionLiveVoiceStopUnstable({ sessionId, interactionId });
}
