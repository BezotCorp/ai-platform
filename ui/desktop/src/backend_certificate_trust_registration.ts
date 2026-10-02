import { BackendCertificateTrust } from './backend_certificate_trust';

export interface BackendCertificateTrustRegistration {
  trust: BackendCertificateTrust;
  release: () => void;
}
