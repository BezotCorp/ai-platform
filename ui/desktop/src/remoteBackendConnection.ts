import type { RemoteBackendStep } from './remote_backend_step';

export interface RemoteBackendConnection {
  ok: boolean;
  steps: RemoteBackendStep[];
  failure: string | null;
  acpUrl: string | null;
}
