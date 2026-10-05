import type { GooseServeStartupDiagnostics } from './gooseServeStartupDiagnostics';

export interface GooseServeStartupTrace {
  diagnosticsPath: string;
  diagnostics: GooseServeStartupDiagnostics;
  record: (name: string, details?: Record<string, unknown>) => void;
  flush: () => void;
}
