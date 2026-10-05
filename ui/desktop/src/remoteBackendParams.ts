import type { HopRequest } from './hopRequest';

export interface RemoteBackendParams {
  baseUrl: string;
  serverSecret: string;
  pinnedHostname?: string | null;
  errorLog?: string[];
  request?: HopRequest;
}
