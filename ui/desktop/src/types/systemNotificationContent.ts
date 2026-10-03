import type { SystemNotificationType } from '.';

export type SystemNotificationContent = {
  data?: unknown;
  msg: string;
  notificationType: SystemNotificationType;
};
