import { type ChildProcess } from 'child_process';
import type { GooseServeExitSignal } from './gooseServeExitSignal';
import type { GooseServeStartupDiagnostics } from './gooseServeStatupDiagnostics';

export interface GooseServeResult {
  acpUrl: string;
  workingDir: string;
  process: ChildProcess;
  errorLog: string[];
  certFingerprint: string | null;
  cleanup: () => Promise<void>;
  hasExited: () => boolean;
  getExitDetails: () => { code: number | null; signal: GooseServeExitSignal };
  startupDiagnosticsPath: string | null;
  getStartupDiagnostics: () => GooseServeStartupDiagnostics | null;
  recordStartupEvent: (name: string, details?: Record<string, unknown>) => void;
}
