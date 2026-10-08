import type { Session } from './types/session';
import { configuredExtensionEntryToConfig } from './utils/configuredExtensionEntryToConfig';
import type { ExtensionConfig } from './types/extensionConfig';
import type { BcaipExtension } from '@bezotcorp/bcaip-acp-client';
import type { SetViewType } from './hooks/useNavigation';

import { AppEvents } from './constants/appEvents';
import { acpChatSessionController } from './acp/chatSessionController';
import { getConfiguredBcaipExtensions, bcaipExtensionName } from './acp/extensions';
import { beginConfiguredRecipeParameterScope } from './acp/recipeParamRequests';
import { getAcpFeatureCapabilities } from './acp/capabilities';
import { RecipeDeclinedError, RecipeParameterScopesUnsupportedError } from './acp/errors';
import { decodeRecipe } from './acp/recipe';
import { scanRecipe, type Recipe } from './recipe';
import { listSavedRecipes } from './recipe/recipe_management';
import { requestRecipeConsent } from './recipe/consent';
import { CreateSessionOptions } from './createSessionOptions';
import type { ConfiguredExtensionEntry } from './types/configuredExtensionEntry';

export function getSessionDisplayName(session: Session): string {
  if (session.user_set_name) {
    return session.name;
  }
  if (session.recipe?.title) {
    return session.recipe.title;
  }
  return session.name;
}

/**
 * Three-valued on purpose. `undefined` means the caller is not naming a set and the
 * backend should use the configured one; `[]` means the user asked for a session with
 * no extensions at all. Collapsing the two is what made an all-off selection come back
 * with the default extensions.
 */
function selectedExtensionConfigs(options?: CreateSessionOptions): ExtensionConfig[] | undefined {
  if (options?.extensionConfigs) {
    return options.extensionConfigs;
  }
  if (options?.allExtensions) {
    const enabled = options.allExtensions
      .filter((extension) => extension.enabled)
      .map(configuredExtensionEntryToConfig);
    // An empty configured list is also what this looks like before the config
    // finishes loading, so it stays "not specified" rather than becoming an
    // explicit empty selection. Only `extensionConfigs` can express that.
    return enabled.length > 0 ? enabled : undefined;
  }
  return undefined;
}

async function resolveBcaipExtensions(
  selected: ExtensionConfig[] | undefined
): Promise<BcaipExtension[] | undefined> {
  if (selected === undefined) {
    return undefined;
  }
  if (selected.length === 0) {
    return [];
  }
  const selectedNames = new Set(selected.map((config) => config.name));
  return (await getConfiguredBcaipExtensions())
    .filter((entry) => selectedNames.has(bcaipExtensionName(entry.extension)))
    .map((entry) => entry.extension);
}

async function resolveRecipe(options?: CreateSessionOptions): Promise<Recipe | undefined> {
  if (options?.recipeId) {
    const entry = (await listSavedRecipes()).find((manifest) => manifest.id === options.recipeId);
    if (!entry) {
      throw new Error(`Recipe ${options.recipeId} was not found in the recipe library`);
    }
    return entry.recipe;
  }
  if (options?.recipeDeeplink) {
    return decodeRecipe(options.recipeDeeplink);
  }
  return undefined;
}

// Recipes can declare commands, endpoints, and shell checks that run as soon as the
// session exists, so consent has to be settled before session/new is ever sent.
async function ensureRecipeConsent(options?: CreateSessionOptions): Promise<void> {
  const recipe = await resolveRecipe(options);
  if (!recipe || (await window.electron.hasAcceptedRecipeBefore(recipe))) {
    return;
  }

  const scan = await scanRecipe(recipe);
  const accepted = await requestRecipeConsent({
    recipe,
    hasSecurityWarnings: scan.has_security_warnings,
  });
  if (!accepted) {
    throw new RecipeDeclinedError();
  }
  await window.electron.recordRecipeHash(recipe);
}

async function createAcpSession(
  workingDir: string,
  options?: CreateSessionOptions
): Promise<Session> {
  await ensureRecipeConsent(options);

  const configuredParameterScope = options?.recipeDeeplink
    ? beginConfiguredRecipeParameterScope()
    : undefined;
  try {
    if (configuredParameterScope) {
      const capabilities = await getAcpFeatureCapabilities();
      if (!capabilities.recipeParameterScopes) {
        throw new RecipeParameterScopesUnsupportedError();
      }
    }
    const bcaipExtensions = await resolveBcaipExtensions(selectedExtensionConfigs(options));
    return await acpChatSessionController.createSession(workingDir, bcaipExtensions, {
      recipeId: options?.recipeId,
      recipeDeeplink: options?.recipeDeeplink,
      recipeParameterScopeId: configuredParameterScope?.id,
    });
  } finally {
    configuredParameterScope?.finish();
  }
}

export async function createSession(
  workingDir: string,
  options?: CreateSessionOptions
): Promise<Session> {
  return createAcpSession(workingDir, options);
}

export async function startNewSession(
  initialText: string | undefined,
  setView: SetViewType,
  workingDir: string,
  options?: {
    recipeDeeplink?: string;
    recipeId?: string;
    allExtensions?: ConfiguredExtensionEntry[];
  }
): Promise<Session> {
  const session = await createSession(workingDir, options);
  window.dispatchEvent(new CustomEvent(AppEvents.SESSION_CREATED, { detail: { session } }));

  const initialMessage = initialText ? { msg: initialText, images: [] } : undefined;

  const eventDetail = {
    sessionId: session.id,
    initialMessage,
  };

  window.dispatchEvent(
    new CustomEvent(AppEvents.ADD_ACTIVE_SESSION, {
      detail: eventDetail,
    })
  );

  setView('pair', {
    disableAnimation: true,
    initialMessage,
    resumeSessionId: session.id,
  });
  return session;
}
