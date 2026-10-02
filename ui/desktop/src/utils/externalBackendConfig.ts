export interface ExternalBackendConfig {
  enabled: boolean;
  url: string;
  secret: string;
  certFingerprint?: string;
  workingDir?: string;
}
