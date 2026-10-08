import type { AcpElicitationRequest } from './acpElicitationRequest';
import type { BcaipSessionNotificationUnstable } from '@bezotcorp/bcaip-acp-client';
import type { SessionNotification } from '@agentclientprotocol/sdk';
import type { Message } from '../types/message';
import { type ElicitationStatus } from './adapter/elicitationStatus';
import { type AcpChatStateChange } from './adapter/shared';

import type { AcpPermissionRequest } from './acpPermissionRequest';

export interface AcpSessionNotificationAdapter {
  apply(notification: SessionNotification): AcpChatStateChange[];
  applyBcaip(notification: BcaipSessionNotificationUnstable): AcpChatStateChange[];
  applyPermissionRequest(request: AcpPermissionRequest): AcpChatStateChange[];
  cancelPermissionRequest(toolCallId: string, generation: string): AcpChatStateChange[];
  applyElicitationRequest(request: AcpElicitationRequest): AcpChatStateChange[];
  applyElicitationStatus(elicitationId: string, status: ElicitationStatus): AcpChatStateChange[];
  getMessages(): Message[];
}
