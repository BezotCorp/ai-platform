import type { LiveOutputNotificationChunk } from './liveOutputNotificationChunk';

export type LiveOutputNotificationParams = {
  sequence: number;
  chunks: LiveOutputNotificationChunk[];
  truncated: boolean;
};
