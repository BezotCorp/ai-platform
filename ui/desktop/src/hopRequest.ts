import type { HopInit } from './hopInit';
import type { Hop } from './hop';

export type HopRequest = (url: string, init: HopInit) => Promise<Hop>;
