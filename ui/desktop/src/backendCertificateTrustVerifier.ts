export interface BackendCertificateTrustVerifier {
  has(hostname: string): boolean;
  verify(hostname: string, fingerprint: string): boolean;
}
