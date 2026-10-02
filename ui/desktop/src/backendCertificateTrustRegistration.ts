import type { BackendCertificateTrust } from './backendCertificateTrust';

export interface BackendCertificateTrustRegistration {
  trust: BackendCertificateTrust;
  release: () => void;
}
