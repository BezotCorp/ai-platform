export interface StartupTraceEvent {
  name: string;
  at: string;
  elapsedMs: number;
  details?: Record<string, unknown>;
}
