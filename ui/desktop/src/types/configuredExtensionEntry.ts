import type { ExtensionConfig } from './extensionConfig';

export type ConfiguredExtensionEntry = ExtensionConfig & {
  enabled: boolean;
  configKey?: string;
};
