export type ProgressNotificationParams = {
  progressToken: string | number;
  progress: number;
  total?: number;
  message?: string;
};
