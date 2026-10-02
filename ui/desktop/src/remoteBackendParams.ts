import type { HopRequest } from './hotRequest';

export interface RemoteBackendParams {
  baseUrl: string;
  serverSecret: string;
  pinnedHostname?: string | null;
  errorLog?: string[];
  request?: HopRequest;
}
