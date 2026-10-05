import type { SessionScopedFormElicitationRequest } from './sessionScopedFormElicitationRequest';

export interface AcpElicitationRequest {
  id: string;
  sessionId: string;
  request: SessionScopedFormElicitationRequest;
}
