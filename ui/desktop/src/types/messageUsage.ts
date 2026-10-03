/** Mirrors the backend `MessageUsage` schema (camelCase). */
export type MessageUsage = {
  inputTokens?: number | null;
  outputTokens?: number | null;
  totalTokens?: number | null;
  cacheReadTokens?: number | null;
  cacheWriteTokens?: number | null;
  cost?: number | null;
  costSource?: 'provider_reported' | 'estimated' | null;
  elapsedMs?: number | null;
  timeToFirstTokenMs?: number | null;
  isCompaction?: boolean;
};