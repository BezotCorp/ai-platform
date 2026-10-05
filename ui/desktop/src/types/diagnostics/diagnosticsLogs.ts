import type { DiagnosticsTextFile } from './diagnosticsTextFile';

export type DiagnosticsLogs = {
  cli: DiagnosticsTextFile[];
  llm: DiagnosticsTextFile[];
};
