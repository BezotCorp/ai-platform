// Certificate trust for active backend leases. Renderer requests and
// main-process net.fetch both pin to the exact cert fingerprint. Each backend
// lease owns a trust record so old windows keep working after settings change.
export interface BackendCertificateTrust {
  hostname: string;
  fingerprint: string | null;
}
