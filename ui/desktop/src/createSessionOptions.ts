import type { ExtensionConfig } from './types/extensions';
import type { FixedExtensionEntry } from './components/ConfigContext';

export interface CreateSessionOptions {
  recipeDeeplink?: string;
  recipeId?: string;
  extensionConfigs?: ExtensionConfig[];
  allExtensions?: FixedExtensionEntry[];
}
