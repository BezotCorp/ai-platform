import type { PlatformEventData } from './platformEventData';

export type PlatformEventHandler = (eventType: string, data: PlatformEventData) => Promise<void>;
