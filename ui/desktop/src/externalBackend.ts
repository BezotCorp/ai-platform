export interface ExternalBackend {
  source: 'env' | 'settings';
  url: string;
  secret: string;
  certFingerprint?: string;
  workingDir?: string;
}
