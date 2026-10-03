import type { ExtensionConfig } from '../../types/extensionConfig';

export type ExtensionMenuEntry = ExtensionConfig & {
  enabled: boolean;
  configKey?: string;
  extensionKey?: string;
};
