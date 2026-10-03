import type { ConfiguredExtensionEntry } from '../types/configuredExtensionEntry';

export interface ConfiguredExtensionsResponse {
  extensions: ConfiguredExtensionEntry[];
  warnings: string[];
}
