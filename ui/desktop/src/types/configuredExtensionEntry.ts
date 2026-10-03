import type { ExtensionConfig } from '.';

export type ConfiguredExtensionEntry = ExtensionConfig & {
  enabled: boolean;
  configKey?: string;
};
