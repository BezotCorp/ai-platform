import type { StartupTraceEvent } from './startupTraceEvent';

export interface GooseServeStartupDiagnostics {
  attemptId: string;
  startedAt: string;
  binaryPath: string | null;
  workingDir: string;
  httpBaseUrl: string | null;
  readinessUrl: string | null;
  statusUrl: string | null;
  healthUrl: string | null;
  acpUrl: string | null;
  pid: number | null;
  healthCheckSucceeded: boolean;
  childExitCode: number | null;
  childExitSignal: string | null;
  stderrTail: string[];
  events: StartupTraceEvent[];
}
