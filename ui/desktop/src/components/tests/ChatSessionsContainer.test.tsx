/**
 * @vitest-environment jsdom
 */

import { act, render } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { subscribeToAcpRecovery } from '../../acp/acpConnection';
import { acpChatSessionController } from '../../acp/chatSessionController';
import type { LiveVoiceController } from '../../liveVoice/useLiveVoice';
import ChatSessionsContainer from '../ChatSessionsContainer';

const { setSearchParams } = vi.hoisted(() => ({
  setSearchParams: vi.fn(),
}));

vi.mock('react-router', () => ({
  useSearchParams: () => [
    new URLSearchParams('resumeSessionId=session-1'),
    setSearchParams,
  ],
}));

vi.mock('../BaseChat', () => ({
  default: ({ sessionId }: { sessionId: string }) => <div>{sessionId}</div>,
}));

vi.mock('../../acp/acpConnection', () => ({
  subscribeToAcpRecovery: vi.fn(),
}));

vi.mock('../../acp/chatSessionController', () => ({
  acpChatSessionController: {
    restoreSession: vi.fn().mockResolvedValue(undefined),
  },
}));

describe('ChatSessionsContainer', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('restores active chat sessions after ACP reconnects', () => {
    let onRecoveryChanged: ((recovering: boolean) => void) | undefined;

    vi.mocked(subscribeToAcpRecovery).mockImplementation((listener) => {
      onRecoveryChanged = listener;
      return () => undefined;
    });

    const liveVoice = {
      activeSessionId: null,
      liveVoiceSessionId: null,
      phase: 'idle',
      muted: false,
      start: vi.fn(),
      stop: vi.fn(),
      toggleMute: vi.fn(),
    } satisfies LiveVoiceController;

    render(
      <ChatSessionsContainer
        setChat={vi.fn()}
        activeSessions={[
          { sessionId: 'session-1' },
          { sessionId: 'session-2' },
        ]}
        liveVoice={liveVoice}
      />
    );

    expect(subscribeToAcpRecovery).toHaveBeenCalledOnce();
    expect(onRecoveryChanged).toBeDefined();

    act(() => {
      onRecoveryChanged?.(false);
    });

    expect(acpChatSessionController.restoreSession).toHaveBeenCalledTimes(2);
    expect(acpChatSessionController.restoreSession).toHaveBeenNthCalledWith(
      1,
      'session-1'
    );
    expect(acpChatSessionController.restoreSession).toHaveBeenNthCalledWith(
      2,
      'session-2'
    );
  });
});