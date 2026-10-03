import type { ConfiguredExtensionEntry } from '../types/configuredExtensionEntry';
import type { ExtensionConfig } from '../types/extensionConfig';

/**
 * Removes global configuration metadata from a configured extension entry.
 */
export function configuredExtensionEntryToConfig(
  entry: ConfiguredExtensionEntry
): ExtensionConfig {
  const {
    enabled: _enabled,
    configKey: _configKey,
    ...config
  } = entry;

  return config;
}
