import type { ExtensionConfig } from './types/extensions';
import type { ConfiguredExtensionEntry } from './types/configuredExtensionEntry';


export interface CreateSessionOptions {
  recipeDeeplink?: string;
  recipeId?: string;
  extensionConfigs?: ExtensionConfig[];
  allExtensions?: ConfiguredExtensionEntry[];
}
