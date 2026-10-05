import type { ConfiguredExtensionEntry } from '../../types/configuredExtensionEntry';
import type { ExtensionConfig } from '../../types/extensionConfig';
import type { ProviderDetails } from '../../types/providerDetails';
import type { ConfigMap } from './configMap';

export interface ConfigContextType {
  config: ConfigMap;
  providersList: ProviderDetails[];
  extensionsList: ConfiguredExtensionEntry[];
  extensionWarnings: string[];
  upsert: (key: string, value: unknown, is_secret: boolean) => Promise<void>;
  read: (key: string, is_secret: boolean, options?: { throwOnError?: boolean }) => Promise<unknown>;
  remove: (key: string, is_secret: boolean) => Promise<void>;
  addExtension: (name: string, config: ExtensionConfig, enabled: boolean) => Promise<void>;
  setExtensionEnabled: (configKey: string, enabled: boolean) => Promise<void>;
  removeExtension: (name: string) => Promise<void>;
  getProviders: (b: boolean) => Promise<ProviderDetails[]>;
  getExtensions: (b: boolean) => Promise<ConfiguredExtensionEntry[]>;
}
