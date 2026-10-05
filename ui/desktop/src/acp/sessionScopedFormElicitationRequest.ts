import type { CreateElicitationRequest } from '@agentclientprotocol/sdk';
import type { ElicitationSchema } from '@agentclientprotocol/sdk';

export type SessionScopedFormElicitationRequest = CreateElicitationRequest & {
  mode: 'form';
  sessionId: string;
  requestedSchema: ElicitationSchema;
};
