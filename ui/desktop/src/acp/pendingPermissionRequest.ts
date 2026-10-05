import type { RequestPermissionRequest, RequestPermissionResponse } from '@agentclientprotocol/sdk';

export interface PendingPermissionRequest {
  request: RequestPermissionRequest;
  generation: string;
  resolve: (response: RequestPermissionResponse) => void;
}
