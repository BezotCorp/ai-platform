export interface Probe {
  ok: boolean;
  detail: string;
  retryable: boolean;
  resolvedUrl?: string;
}
