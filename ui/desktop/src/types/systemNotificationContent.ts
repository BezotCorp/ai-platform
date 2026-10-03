import type { SystemNotificationType } from './systemNotificationType';

export type SystemNotificationContent = {
  data?: unknown;
  msg: string;
  notificationType: SystemNotificationType;
};
