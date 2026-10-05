


import type { BackendCertificateTrustVerifier } from './backendCertificateTrustVerifier';
import type { CertificateVerifierSession } from './certificateVerifierSession';
export function installBackendCertificateVerifiers(
  targetSessions: CertificateVerifierSession[],
  trustVerifier: BackendCertificateTrustVerifier
): void {
  for (const targetSession of targetSessions) {
    targetSession.setCertificateVerifyProc((request, callback) => {
      if (!trustVerifier.has(request.hostname)) {
        callback(-3);
        return;
      }
      const match = trustVerifier.verify(request.hostname, request.certificate.fingerprint);
      callback(match ? 0 : -2);
    });
  }
}

export type { BackendCertificateTrustVerifier } from './backendCertificateTrustVerifier';
