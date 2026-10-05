import type { LiveOutputNotificationParams } from '../../types/liveOutputNotificationParams';
import type { LoggingMessageNotificationParams } from './loggingMessageNotificationParams';
import type { ProgressNotificationParams } from './progressNotificationParams';
import type { PlatformEventParams } from './platformEventParams';

export type ToolNotification =
  | {
      type: 'message';
      params: LoggingMessageNotificationParams;
    }
  | {
      type: 'progress';
      params: ProgressNotificationParams;
    }
  | {
      type: 'platform_event';
      params: PlatformEventParams;
    }
  | {
      type: 'live_output';
      params: LiveOutputNotificationParams;
    };
