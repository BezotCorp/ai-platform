import type { AcpCreditsExhaustedError } from './acpCreditsExhaustedError';
import type { AcpChatSessionController } from './acpChatSessionController';
export type { AcpChatSessionController } from './acpChatSessionController';

import { v7 as uuidv7 } from 'uuid';
import type { BcaipExtension } from '@bezotcorp/bcaip-acp-client';
import { AppEvents } from '../constants/appEvents';
import { ChatState } from '../types/chatState';
import type { Session } from '../types/session';
import { showExtensionLoadResults } from '../utils/extensionErrorUtils';
import { createUserMessage } from '../types/message';
import type { Message } from '../types/message';
import type { ImageData } from '../types/imageData';
import { getPendingToolConfirmationIds } from '../types/toolConfirmationData';
import { acpChatSessionActions, acpChatSessionStore } from './chatSessionStore';
import { cancelAcpElicitationRequestsForSession } from './elicitationRequests';
import { formatAcpError, parseAcpCreditsExhaustedError } from './errors';
import { cancelAcpPermissionRequestsForSession } from './permissionRequests';
import { acpCancelPrompt, acpPromptSession } from './prompt';
import {
  acpForkSession,
  acpLoadSession,
  acpNewSession,
  acpTruncateSessionConversation,
  isAcpSessionLoadInFlight,
  sessionInfoToSession,
} from '../sessions';
import { AcpRecipeOptions } from './acpRecipeOptions';

import type { AcpLoadSessionOptions } from './acpLoadSessionOptions';
import type { AcpSubmitMessageOptions } from './acpSubmitMessageOptions';

function createAcpCreditsExhaustedMessage(error: AcpCreditsExhaustedError): Message {
  return {
    id: uuidv7(),
    role: 'assistant',
    created: Math.floor(Date.now() / 1000),
    content: [
      {
        type: 'systemNotification',
        notificationType: 'creditsExhausted',
        msg: error.message,
        ...(error.url ? { data: { top_up_url: error.url } } : {}),
      },
    ],
    metadata: { userVisible: true, agentVisible: false },
  };
}

function assertNoPendingPromptCancellation(sessionId: string): void {
  const snapshot = acpChatSessionStore.getSnapshot(sessionId);
  if (snapshot?.pendingCancelPromptAttemptId) {
    throw new Error('Cannot submit while prompt cancellation is pending');
  }
}

async function forkSessionWithEditedMessage(
  sessionId: string,
  message: Message,
  editedMessage: string,
  editedImages: ImageData[]
): Promise<void> {
  const targetSessionId = await acpForkSession(sessionId, message.created);

  const event = new CustomEvent(AppEvents.SESSION_FORKED, {
    detail: {
      newSessionId: targetSessionId,
      shouldStartAgent: true,
      editedMessage,
      editedImages,
    },
  });
  window.dispatchEvent(event);
}

async function createSession(
  cwd: string,
  bcaipExtensions: BcaipExtension[] | undefined,
  recipe?: AcpRecipeOptions
): Promise<Session> {
  const { sessionId, sessionInfo, meta } = await acpNewSession(cwd, bcaipExtensions, recipe);
  const session = sessionInfoToSession(sessionInfo, meta);

  showExtensionLoadResults(meta.extensionResults);
  window.dispatchEvent(
    new CustomEvent(AppEvents.SESSION_EXTENSIONS_LOADED, { detail: { sessionId } })
  );
  acpChatSessionActions.finishSessionLoad(sessionId, session);

  return session;
}

async function loadSession(sessionId: string, options: AcpLoadSessionOptions = {}): Promise<void> {
  const cached = acpChatSessionStore.getSnapshot(sessionId);
  if (cached?.session && !cached.sessionLoadError) {
    window.dispatchEvent(
      new CustomEvent(AppEvents.SESSION_EXTENSIONS_LOADED, { detail: { sessionId } })
    );
    options.onSessionLoaded?.();
    return;
  }

  await loadSessionFromServer(sessionId, options);
}

async function restoreSession(sessionId: string): Promise<void> {
  await loadSessionFromServer(sessionId);
}

async function loadSessionFromServer(
  sessionId: string,
  options: AcpLoadSessionOptions = {}
): Promise<void> {
  if (!isAcpSessionLoadInFlight(sessionId)) {
    acpChatSessionActions.startSessionLoad(sessionId);
  }

  try {
    const { sessionInfo, meta } = await acpLoadSession(sessionId);

    showExtensionLoadResults(meta.extensionResults);
    window.dispatchEvent(
      new CustomEvent(AppEvents.SESSION_EXTENSIONS_LOADED, { detail: { sessionId } })
    );
    acpChatSessionActions.finishSessionLoad(sessionId, sessionInfoToSession(sessionInfo, meta));
    options.onSessionLoaded?.();
  } catch (error) {
    console.error('Failed to load ACP session:', error);
    acpChatSessionActions.failSessionLoad(sessionId, formatAcpError(error));
  }
}

async function submitMessage(
  sessionId: string,
  userMessage: Message,
  options: AcpSubmitMessageOptions
): Promise<void> {
  assertNoPendingPromptCancellation(sessionId);

  const snapshot = acpChatSessionStore.getSnapshot(sessionId);
  if (snapshot?.activePromptAttemptId) {
    return;
  }

  const promptAttemptId = uuidv7();
  acpChatSessionActions.startPromptAttempt(sessionId, promptAttemptId);

  try {
    await acpPromptSession(sessionId, userMessage);
    if (acpChatSessionActions.clearPromptCancellation(sessionId, promptAttemptId)) {
      return;
    }
    if (acpChatSessionActions.finishPromptAttemptIfCurrent(sessionId, promptAttemptId)) {
      void options.onFinish();
    }
  } catch (error) {
    if (acpChatSessionActions.clearPromptCancellation(sessionId, promptAttemptId)) {
      return;
    }

    const creditsExhaustedError = parseAcpCreditsExhaustedError(error);
    if (creditsExhaustedError) {
      if (!acpChatSessionActions.isCurrentPromptAttempt(sessionId, promptAttemptId)) {
        return;
      }

      const messages = [
        ...(options.getCurrentSnapshot()?.messages ?? []),
        createAcpCreditsExhaustedMessage(creditsExhaustedError),
      ];
      acpChatSessionActions.setMessages(sessionId, messages);
      if (acpChatSessionActions.finishPromptAttemptIfCurrent(sessionId, promptAttemptId)) {
        void options.onFinish();
      }
      return;
    }

    const submitError = formatAcpError(error);
    if (acpChatSessionActions.finishPromptAttemptIfCurrent(sessionId, promptAttemptId)) {
      void options.onFinish(submitError);
    }
  }
}

function stop(sessionId: string): void {
  const storedPromptAttemptId = acpChatSessionStore.getSnapshot(sessionId)?.activePromptAttemptId;
  const hasStoredAcpPrompt = storedPromptAttemptId !== null && storedPromptAttemptId !== undefined;

  if (hasStoredAcpPrompt) {
    acpChatSessionActions.startPromptCancellation(sessionId, storedPromptAttemptId);
    cancelAcpPermissionRequestsForSession(sessionId);
    cancelAcpElicitationRequestsForSession(sessionId);
    acpCancelPrompt(sessionId).catch((error) => {
      console.warn('Failed to cancel ACP prompt:', error);
    });
    return;
  }

  acpChatSessionActions.setChatState(sessionId, ChatState.Idle);
}

async function updateMessage(
  sessionId: string,
  messageId: string,
  newContent: string,
  editType: 'fork' | 'edit',
  retainedImages: ImageData[],
  options: AcpSubmitMessageOptions
): Promise<void> {
  assertNoPendingPromptCancellation(sessionId);

  const currentSnapshot = options.getCurrentSnapshot();
  const storedSnapshot = acpChatSessionStore.getSnapshot(sessionId);
  const activePromptAttemptId = storedSnapshot?.activePromptAttemptId;
  const currentMessages = currentSnapshot?.messages ?? [];
  const message = currentMessages.find((m) => m.id === messageId);

  if (!message) {
    throw new Error(`Message with id ${messageId} not found in current messages`);
  }

  if (editType === 'fork') {
    await forkSessionWithEditedMessage(sessionId, message, newContent, retainedImages);
    return;
  }

  const editSnapshot = currentSnapshot ?? storedSnapshot;
  const isPendingToolPermission =
    editSnapshot?.chatState === ChatState.WaitingForUserInput &&
    getPendingToolConfirmationIds(editSnapshot?.messages ?? []).size > 0;
  const isIdle = editSnapshot?.chatState === ChatState.Idle;
  const pendingToolPermissionPromptAttemptId = isPendingToolPermission
    ? activePromptAttemptId
    : undefined;
  const canEditInPlace = isIdle || pendingToolPermissionPromptAttemptId != null;

  if (!canEditInPlace) {
    return;
  }

  if (pendingToolPermissionPromptAttemptId != null) {
    const cancellation = acpChatSessionActions.startPromptCancellation(
      sessionId,
      pendingToolPermissionPromptAttemptId
    );
    if (!cancellation) {
      throw new Error('Cannot update message while prompt is active');
    }

    const promptCancellationSettled = acpChatSessionActions.waitForPromptCancellation(
      sessionId,
      pendingToolPermissionPromptAttemptId
    );

    try {
      await acpCancelPrompt(sessionId);
    } catch {
      acpChatSessionActions.restorePromptCancellation(
        sessionId,
        pendingToolPermissionPromptAttemptId
      );
      throw new Error('Cannot update message because the active prompt could not be cancelled');
    }

    cancelAcpPermissionRequestsForSession(sessionId);
    cancelAcpElicitationRequestsForSession(sessionId);
    await promptCancellationSettled;
  }

  acpChatSessionActions.setChatState(sessionId, ChatState.Thinking);

  try {
    await acpTruncateSessionConversation(sessionId, message.created);

    const truncatedMessages = currentMessages.filter((m) => m.created < message.created);
    const updatedUserMessage = createUserMessage(newContent, retainedImages);

    const messagesForUI = [...truncatedMessages, updatedUserMessage];
    acpChatSessionActions.setMessages(sessionId, messagesForUI);

    await submitMessage(sessionId, updatedUserMessage, options);
  } catch (error) {
    acpChatSessionActions.setChatState(sessionId, ChatState.Idle);
    throw error;
  }
}

export const acpChatSessionController: AcpChatSessionController = {
  createSession,
  loadSession,
  restoreSession,
  submitMessage,
  stop,
  updateMessage,
};
