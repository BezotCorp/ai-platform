import type { PlatformEventData } from './platformEventData';

export interface AppsEventData extends PlatformEventData {
  app_name?: string;
  sessionId: string;
}
