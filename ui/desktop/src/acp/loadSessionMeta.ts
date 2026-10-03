import type { Recipe } from '../recipe';
import type { ExtensionLoadResult } from '../types/extensionLoadResult';

/**
 * Application metadata recovered while loading an ACP session.
 */
export interface LoadSessionMeta {
  recipe?: Recipe | null;
  userRecipeValues?: Record<string, string> | null;
  extensionResults?: ExtensionLoadResult[] | null;
  workingDir?: string;
}
