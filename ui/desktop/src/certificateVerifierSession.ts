import type { Session } from 'electron';

export type CertificateVerifierSession = Pick<Session, 'setCertificateVerifyProc'>;
