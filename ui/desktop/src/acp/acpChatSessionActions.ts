import type { AcpElicitationRequest } from './acpElicitationRequest';
import type { BcaipSessionNotificationUnstable } from '@bezotcorp/bcaip-acp-client';
import type { SessionNotification } from '@agentclientprotocol/sdk';
import type { ChatState } from '../types/chatState';
import type { Message } from '../types/message';
import type { Session } from '../types/session';
import type { ElicitationStatus } from './adapter/elicitationStatus';

import type { AcpPermissionRequest } from './acpPermissionRequest';
import type { AcpChatSessionSnapshot } from './acpChatSessionSnapshot';

export interface AcpChatSessionActions {
  deleteSnapshot(sessionId: string): void;
  applyAcpSessionNotification(notification: SessionNotification): AcpChatSessionSnapshot;
  applyAcpBcaipSessionNotification(
    notification: BcaipSessionNotificationUnstable
  ): AcpChatSessionSnapshot;
  applyPermissionRequest(request: AcpPermissionRequest): AcpChatSessionSnapshot;
  cancelPermissionRequest(
    sessionId: string,
    toolCallId: string,
    generation: string
  ): AcpChatSessionSnapshot | undefined;
  applyElicitationRequest(request: AcpElicitationRequest): AcpChatSessionSnapshot;
  setElicitationStatus(
    sessionId: string,
    elicitationId: string,
    status: ElicitationStatus
  ): AcpChatSessionSnapshot | undefined;
  setSessionMetadata(sessionId: string, session: Session | undefined): AcpChatSessionSnapshot;
  startSessionLoad(sessionId: string): AcpChatSessionSnapshot;
  finishSessionLoad(sessionId: string, session: Session): AcpChatSessionSnapshot;
  failSessionLoad(sessionId: string, sessionLoadError: string): AcpChatSessionSnapshot;
  setSessionLoadError(
    sessionId: string,
    sessionLoadError: string | undefined
  ): AcpChatSessionSnapshot;
  setMessages(sessionId: string, messages: Message[]): AcpChatSessionSnapshot;
  addPendingLocalSteerMessage(sessionId: string, message: Message): AcpChatSessionSnapshot;
  setChatState(sessionId: string, chatState: ChatState): AcpChatSessionSnapshot;
  resolveUserInputRequest(
    sessionId: string,
    userInputRequestId: string
  ): AcpChatSessionSnapshot | undefined;
  startPromptAttempt(sessionId: string, promptAttemptId: string): AcpChatSessionSnapshot;
  startPromptCancellation(
    sessionId: string,
    promptAttemptId: string
  ): AcpChatSessionSnapshot | undefined;
  clearPromptCancellation(
    sessionId: string,
    promptAttemptId: string
  ): AcpChatSessionSnapshot | undefined;
  restorePromptCancellation(
    sessionId: string,
    promptAttemptId: string
  ): AcpChatSessionSnapshot | undefined;
  waitForPromptCancellation(sessionId: string, promptAttemptId: string): Promise<void>;
  finishPromptAttemptIfCurrent(sessionId: string, promptAttemptId: string): boolean;
  clearActivePromptAttempt(sessionId: string): AcpChatSessionSnapshot | undefined;
  isCurrentPromptAttempt(sessionId: string, promptAttemptId: string): boolean;
}
