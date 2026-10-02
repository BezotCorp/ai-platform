import type { GooseServeExitSignal } from './gooseServeExitSignal';

export interface GooseServeLease {
  acpUrl: string;
  secretKey: string;
  cleanup: () => Promise<void>;
  windowIds: Set<number>;
  cleanedUp: boolean;
  exited: boolean;
  exitCode: number | null;
  exitSignal: GooseServeExitSignal;
}
