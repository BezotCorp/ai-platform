import type { CreateElicitationResponse } from '@agentclientprotocol/sdk';
import type { AcpElicitationRequest } from './acpElicitationRequest';

export interface PendingElicitationRequest {
  request: AcpElicitationRequest;
  resolve: (response: CreateElicitationResponse) => void;
  timeoutId: ReturnType<typeof setTimeout>;
}
