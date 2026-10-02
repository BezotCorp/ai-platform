import type { RemoteBackendStep } from './remoteBackendStep';

export interface RemoteBackendConnection {
  ok: boolean;
  steps: RemoteBackendStep[];
  failure: string | null;
  acpUrl: string | null;
}
