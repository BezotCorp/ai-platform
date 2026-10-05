import type { DiagnosticsLevel } from './diagnosticsLevel';
import type { SystemInfo } from './systemInfo';
import type { DiagnosticsConfig } from './diagnosticsConfig';
import type { DiagnosticsError } from './diagnosticsError';
import type { DiagnosticsExtensions } from './diagnosticsExtensions';
import type { DiagnosticsLogs } from './diagnosticsLogs';
import type { DiagnosticsPrompt } from './diagnosticsPrompt';
import type { DiagnosticsScheduledRecipe } from './diagnosticsScheduledRecipe';

export type DiagnosticsReport = {
  config?: DiagnosticsConfig | null;
  errors: DiagnosticsError[];
  extensions: DiagnosticsExtensions;
  generatedAt: string;
  level: DiagnosticsLevel;
  logs: DiagnosticsLogs;
  prompts: DiagnosticsPrompt[];
  schedule?: unknown;
  scheduledRecipes: DiagnosticsScheduledRecipe[];
  schemaVersion: number;
  session?: unknown;
  system: SystemInfo;
};
