/**
 * @vitest-environment jsdom
 */

import { act, render, waitFor } from '@testing-library/react';
import type { ComponentProps } from 'react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { subscribeToAcpRecovery } from '../../acp/acpConnection';
import { acpGetLiveVoiceAvailability } from '../../acp/liveVoice';
import { AppEvents } from '../../constants/events';
import { IntlTestWrapper } from '../../i18n/test-utils';
import type { LiveVoiceController } from '../../liveVoice/useLiveVoice';
import { createSession } from '../../sessions';
import type { UserInput } from '../../types/userInput';
import { Session as AppSession } from '../../types/session';
import Hub from '../Hub';

type ChatInputProps = {
  draftRef?: { current: string };
  handleSubmit: (input: UserInput) => Promise<void>;
  liveVoice?: {
    availability: { status: string; message: string } | null;
    start: () => Promise<void>;
  };
  onNextChatExtensionDraftChange?: (draft: { selectedNames: Set<string> }) => void;
};

type CreatedSession = Awaited<ReturnType<typeof createSession>>;
type SetView = ComponentProps<typeof Hub>['setView'];

const captured = vi.hoisted(() => ({
  chatInput: null as ChatInputProps | null,
}));

vi.mock('../ChatInput', () => ({
  default: (props: ChatInputProps) => {
    captured.chatInput = props;
    return <div data-testid="chat-input" />;
  },
}));

vi.mock('../LoadingGoose', () => ({
  default: () => <div />,
}));

vi.mock('../ConfigContext', () => ({
  useConfig: () => ({
    extensionsList: [],
  }),
}));

vi.mock('../../sessions', () => ({
  createSession: vi.fn(),
}));

vi.mock('../../utils/workingDir', () => ({
  getInitialWorkingDir: () => '/tmp/goose',
  getEffectiveWorkingDir: () => Promise.resolve('/tmp/goose'),
}));

vi.mock('../../utils/nextChatExtensions', () => ({
  createNextChatExtensionDraft: () => ({
    selectedNames: new Set<string>(),
  }),
  selectNextChatExtensions: () => [],
}));

vi.mock('../../acp/errors', () => ({
  formatAcpError: (error: unknown) => String(error),
}));

vi.mock('../../toastService', () => ({
  toastError: vi.fn(),
}));

vi.mock('../../acp/liveVoice', () => ({
  acpGetLiveVoiceAvailability: vi.fn(),
}));

vi.mock('../../acp/acpConnection', () => ({
  subscribeToAcpRecovery: vi.fn(),
}));

const DRAFT = 'a half-written thought';
const TYPED_WHILE_STARTING = 'and one more thought';

function createSessionResult(id = 'session-1'): AppSession {
  return new AppSession({
    id,
    name: 'untitled',
    message_count: 0,
    created_at: '2026-01-01T00:00:00.000Z',
    updated_at: '2026-01-01T00:00:00.000Z',
    working_dir: '/tmp/goose',
    extension_data: {},
  });
}

function createLiveVoice(): LiveVoiceController {
  return {
    activeSessionId: null,
    liveVoiceSessionId: null,
    phase: 'idle',
    muted: false,
    start: vi.fn(),
    stop: vi.fn(),
    toggleMute: vi.fn(),
  };
}

/**
 * Provides a real pending Promise to the mocked createSession dependency.
 *
 * resolve/reject exist immediately. The helper does not reproduce any Hub
 * behaviour: Hub still decides when and how createSession is called.
 */
function createPendingSession() {
  let resolve!: (session: CreatedSession) => void;
  let reject!: (error: Error) => void;

  const promise = new Promise<CreatedSession>((promiseResolve, promiseReject) => {
    resolve = promiseResolve;
    reject = promiseReject;
  });

  vi.mocked(createSession).mockReturnValue(promise);

  return {
    resolve,
    reject,
  };
}

function getChatInput(): ChatInputProps {
  if (!captured.chatInput) {
    throw new Error('Hub did not render ChatInput');
  }

  return captured.chatInput;
}

describe('Hub', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    captured.chatInput = null;

    vi.mocked(acpGetLiveVoiceAvailability).mockRejectedValue(
      new Error('ACP unavailable')
    );

    vi.mocked(subscribeToAcpRecovery).mockReturnValue(() => undefined);
  });

  it('requests Live voice availability again after ACP recovery finishes', async () => {
    let recoveryChanged: ((recovering: boolean) => void) | undefined;

    const availability = {
      status: 'ready' as const,
      message: 'Start Live voice',
    };

    vi.mocked(acpGetLiveVoiceAvailability)
      .mockRejectedValueOnce(new Error('ACP disconnected'))
      .mockResolvedValueOnce(availability);

    vi.mocked(subscribeToAcpRecovery).mockImplementation((listener) => {
      recoveryChanged = listener;
      return () => undefined;
    });

    render(
      <IntlTestWrapper>
        <Hub
          setView={vi.fn()}
          draftRef={{ current: '' }}
          liveVoice={createLiveVoice()}
        />
      </IntlTestWrapper>
    );

    await waitFor(() => {
      expect(acpGetLiveVoiceAvailability).toHaveBeenCalledTimes(1);
    });

    expect(recoveryChanged).toBeDefined();

    act(() => {
      recoveryChanged!(true);
    });

    expect(acpGetLiveVoiceAvailability).toHaveBeenCalledTimes(1);

    act(() => {
      recoveryChanged!(false);
    });

    await waitFor(() => {
      expect(acpGetLiveVoiceAvailability).toHaveBeenCalledTimes(2);
      expect(getChatInput().liveVoice?.availability).toEqual(availability);
    });
  });

  it('returns to the session that already owns the Live voice interaction', async () => {
    const setView = vi.fn<SetView>();
    const liveVoice = createLiveVoice();

    liveVoice.activeSessionId = 'session-with-live-voice';

    render(
      <IntlTestWrapper>
        <Hub
          setView={setView}
          draftRef={{ current: '' }}
          liveVoice={liveVoice}
        />
      </IntlTestWrapper>
    );

    const startLiveVoice = getChatInput().liveVoice?.start;

    expect(startLiveVoice).toBeDefined();

    await act(async () => {
      await startLiveVoice!();
    });

    expect(setView).toHaveBeenCalledWith('pair', {
      resumeSessionId: 'session-with-live-voice',
    });

    expect(createSession).not.toHaveBeenCalled();
  });

  it('creates a chat with no extensions when the picker was explicitly cleared', async () => {
    vi.mocked(createSession).mockResolvedValue(createSessionResult());

    render(
      <IntlTestWrapper>
        <Hub
          setView={vi.fn()}
          draftRef={{ current: DRAFT }}
          liveVoice={createLiveVoice()}
        />
      </IntlTestWrapper>
    );

    expect(getChatInput().onNextChatExtensionDraftChange).toBeDefined();

    act(() => {
      getChatInput().onNextChatExtensionDraftChange!({
        selectedNames: new Set(),
      });
    });

    /*
     * The state change above re-renders Hub and therefore creates a new
     * handleSubmit closure. Read ChatInput again instead of using props from
     * the previous render.
     */
    await act(async () => {
      await getChatInput().handleSubmit({
        msg: DRAFT,
        images: [],
      });
    });

    expect(createSession).toHaveBeenCalledWith('/tmp/goose', {
      extensionConfigs: [],
    });
  });

  it('leaves extensions unspecified when the picker was never touched', async () => {
    vi.mocked(createSession).mockResolvedValue(createSessionResult());

    render(
      <IntlTestWrapper>
        <Hub
          setView={vi.fn()}
          draftRef={{ current: DRAFT }}
          liveVoice={createLiveVoice()}
        />
      </IntlTestWrapper>
    );

    await act(async () => {
      await getChatInput().handleSubmit({
        msg: DRAFT,
        images: [],
      });
    });

    expect(createSession).toHaveBeenCalledWith('/tmp/goose', {
      allExtensions: [],
    });
  });

  it('passes the real draft ref to ChatInput', () => {
    const draftRef = {
      current: DRAFT,
    };

    render(
      <IntlTestWrapper>
        <Hub
          setView={vi.fn()}
          draftRef={draftRef}
          liveVoice={createLiveVoice()}
        />
      </IntlTestWrapper>
    );

    expect(getChatInput().draftRef).toBe(draftRef);
  });

  it('clears an unchanged draft after the session starts', async () => {
    const pending = createPendingSession();
    const draftRef = {
      current: DRAFT,
    };

    render(
      <IntlTestWrapper>
        <Hub
          setView={vi.fn()}
          draftRef={draftRef}
          liveVoice={createLiveVoice()}
        />
      </IntlTestWrapper>
    );

    let submission!: Promise<void>;

    act(() => {
      submission = getChatInput().handleSubmit({
        msg: DRAFT,
        images: [],
      });
    });

    await waitFor(() => {
      expect(createSession).toHaveBeenCalledOnce();
    });

    await act(async () => {
      pending.resolve(createSessionResult());
      await submission;
    });

    expect(draftRef.current).toBe('');
  });

  it('keeps the draft when session creation fails', async () => {
    const pending = createPendingSession();
    const draftRef = {
      current: DRAFT,
    };

    render(
      <IntlTestWrapper>
        <Hub
          setView={vi.fn()}
          draftRef={draftRef}
          liveVoice={createLiveVoice()}
        />
      </IntlTestWrapper>
    );

    let submission!: Promise<void>;

    act(() => {
      submission = getChatInput().handleSubmit({
        msg: DRAFT,
        images: [],
      });
    });

    await waitFor(() => {
      expect(createSession).toHaveBeenCalledOnce();
    });

    await act(async () => {
      pending.reject(new Error('no agent'));
      await submission;
    });

    expect(draftRef.current).toBe(DRAFT);
  });

  it('preserves text typed while session creation is pending', async () => {
    const pending = createPendingSession();
    const draftRef = {
      current: DRAFT,
    };

    render(
      <IntlTestWrapper>
        <Hub
          setView={vi.fn()}
          draftRef={draftRef}
          liveVoice={createLiveVoice()}
        />
      </IntlTestWrapper>
    );

    let submission!: Promise<void>;

    act(() => {
      submission = getChatInput().handleSubmit({
        msg: DRAFT,
        images: [],
      });
    });

    await waitFor(() => {
      expect(createSession).toHaveBeenCalledOnce();
    });

    draftRef.current = TYPED_WHILE_STARTING;

    await act(async () => {
      pending.resolve(createSessionResult());
      await submission;
    });

    expect(draftRef.current).toBe(TYPED_WHILE_STARTING);
  });

  it('preserves text typed while a failing session creation is pending', async () => {
    const pending = createPendingSession();
    const draftRef = {
      current: DRAFT,
    };

    render(
      <IntlTestWrapper>
        <Hub
          setView={vi.fn()}
          draftRef={draftRef}
          liveVoice={createLiveVoice()}
        />
      </IntlTestWrapper>
    );

    let submission!: Promise<void>;

    act(() => {
      submission = getChatInput().handleSubmit({
        msg: DRAFT,
        images: [],
      });
    });

    await waitFor(() => {
      expect(createSession).toHaveBeenCalledOnce();
    });

    draftRef.current = TYPED_WHILE_STARTING;

    await act(async () => {
      pending.reject(new Error('no agent'));
      await submission;
    });

    expect(draftRef.current).toBe(TYPED_WHILE_STARTING);
  });

  it('keeps the draft empty when it is cleared while session creation is pending', async () => {
    const pending = createPendingSession();
    const draftRef = {
      current: DRAFT,
    };

    render(
      <IntlTestWrapper>
        <Hub
          setView={vi.fn()}
          draftRef={draftRef}
          liveVoice={createLiveVoice()}
        />
      </IntlTestWrapper>
    );

    let submission!: Promise<void>;

    act(() => {
      submission = getChatInput().handleSubmit({
        msg: DRAFT,
        images: [],
      });
    });

    await waitFor(() => {
      expect(createSession).toHaveBeenCalledOnce();
    });

    draftRef.current = '';

    await act(async () => {
      pending.reject(new Error('no agent'));
      await submission;
    });

    expect(draftRef.current).toBe('');
  });

  it('dispatches session events and navigates after a successful submission', async () => {
    const setView = vi.fn<SetView>();
    const sessionCreated = vi.fn();
    const activeSessionAdded = vi.fn();

    window.addEventListener(AppEvents.SESSION_CREATED, sessionCreated);
    window.addEventListener(AppEvents.ADD_ACTIVE_SESSION, activeSessionAdded);

    try {
      vi.mocked(createSession).mockResolvedValue(
        createSessionResult('created-session')
      );

      const draftRef = {
        current: DRAFT,
      };

      render(
        <IntlTestWrapper>
          <Hub
            setView={setView}
            draftRef={draftRef}
            liveVoice={createLiveVoice()}
          />
        </IntlTestWrapper>
      );

      await act(async () => {
        await getChatInput().handleSubmit({
          msg: DRAFT,
          images: [],
        });
      });

      expect(sessionCreated).toHaveBeenCalledOnce();
      expect(activeSessionAdded).toHaveBeenCalledOnce();

      const event = activeSessionAdded.mock.calls[0][0];

      expect(event).toBeInstanceOf(CustomEvent);

      expect((event as CustomEvent).detail).toEqual({
        sessionId: 'created-session',
        initialMessage: {
          msg: DRAFT,
          images: [],
        },
      });

      expect(setView).toHaveBeenCalledWith('pair', {
        disableAnimation: true,
        resumeSessionId: 'created-session',
        initialMessage: {
          msg: DRAFT,
          images: [],
        },
      });

      expect(draftRef.current).toBe('');
    } finally {
      window.removeEventListener(AppEvents.SESSION_CREATED, sessionCreated);
      window.removeEventListener(AppEvents.ADD_ACTIVE_SESSION, activeSessionAdded);
    }
  });
});