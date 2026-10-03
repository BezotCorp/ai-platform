export type LiveOutputNotificationChunk = {
  stream: 'stdout' | 'stderr';
  output: string;
};
