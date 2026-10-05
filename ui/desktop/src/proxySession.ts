import type { Session } from 'electron';

export type ProxySession = Pick<Session, 'setProxy'>;
