import type { GooseServeStartupDiagnostics } from './gooseServeStatupDiagnostics';

export interface GooseServeStartupTrace {
  diagnosticsPath: string;
  diagnostics: GooseServeStartupDiagnostics;
  record: (name: string, details?: Record<string, unknown>) => void;
  flush: () => void;
}
