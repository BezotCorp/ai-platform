import type { LiveOutputNotificationChunk } from '.';

export type LiveOutputNotificationParams = {
  sequence: number;
  chunks: LiveOutputNotificationChunk[];
  truncated: boolean;
};
