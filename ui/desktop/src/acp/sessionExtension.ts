import type { ExtensionConfig } from '../types/extensionConfig';

export type SessionExtension = ExtensionConfig & {
  extensionKey: string;
};
