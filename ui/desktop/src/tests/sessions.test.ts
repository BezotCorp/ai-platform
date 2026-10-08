/**
 * @vitest-environment jsdom
 */

import type { BcaipExtension, BcaipExtensionEntry } from '@bezotcorp/bcaip-acp-client';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { AppEvents } from '../constants/events';

import type { Recipe } from '../recipe';
import { createSession, getSessionDisplayName, startNewSession } from '../sessions';
import type { ExtensionConfig } from '../types/extensionConfig';
import { Session } from '../types/session';
import type { SessionData } from '../types/sessionData';
import type { ConfiguredExtensionEntry } from '../types/configuredExtensionEntry';

const mocks = vi.hoisted(() => ({
  createAcpSession: vi.fn(),
  getConfiguredBcaipExtensions: vi.fn(),
  beginConfiguredRecipeParameterScope: vi.fn(),
  finishConfiguredRecipeParameterScope: vi.fn(),
  getAcpFeatureCapabilities: vi.fn(),
  decodeRecipe: vi.fn(),
  scanRecipe: vi.fn(),
  listSavedRecipes: vi.fn(),
  requestRecipeConsent: vi.fn(),
  hasAcceptedRecipeBefore: vi.fn(),
  recordRecipeHash: vi.fn(),
}));

vi.mock('../acp/chatSessionController', () => ({
  acpChatSessionController: {
    createSession: mocks.createAcpSession,
  },
}));

vi.mock('../acp/extensions', () => ({
  getConfiguredBcaipExtensions: mocks.getConfiguredBcaipExtensions,
  bcaipExtensionName: (extension: BcaipExtension) => {
    if ('name' in extension) {
      return extension.name;
    }

    return extension.server.name;
  },
}));

vi.mock('../acp/recipeParamRequests', () => ({
  beginConfiguredRecipeParameterScope: mocks.beginConfiguredRecipeParameterScope,
}));

vi.mock('../acp/capabilities', () => ({
  getAcpFeatureCapabilities: mocks.getAcpFeatureCapabilities,
}));

vi.mock('../acp/recipe', () => ({
  decodeRecipe: mocks.decodeRecipe,
}));

vi.mock('../recipe', () => ({
  scanRecipe: mocks.scanRecipe,
}));

vi.mock('../recipe/recipe_management', () => ({
  listSavedRecipes: mocks.listSavedRecipes,
}));

vi.mock('../recipe/consent', () => ({
  requestRecipeConsent: mocks.requestRecipeConsent,
}));

const CREATED_AT = '2026-01-01T00:00:00.000Z';
const UPDATED_AT = '2026-01-01T00:01:00.000Z';

const RECIPE: Recipe = {
  title: 'Test Recipe',
  description: 'Recipe used by sessions tests',
};

function makeSession(overrides: Partial<SessionData> = {}): Session {
  return new Session({
    id: 'session-1',
    name: 'untitled',
    message_count: 0,
    created_at: CREATED_AT,
    updated_at: UPDATED_AT,
    working_dir: '/tmp',
    extension_data: {
      active: [],
      installed: [],
    },
    ...overrides,
  });
}

function extensionConfig(name: string): ExtensionConfig {
  return {
    name,
    type: 'builtin',
    description: `${name} extension`,
  };
}

function configuredExtension(name: string, enabled: boolean): ConfiguredExtensionEntry {
  return {
    ...extensionConfig(name),
    enabled,
  };
}

function gooseExtension(name: string): BcaipExtension {
  return {
    type: 'builtin',
    name,
    description: `${name} extension`,
  };
}

function gooseExtensionEntry(name: string): BcaipExtensionEntry {
  return {
    extension: gooseExtension(name),
    enabled: true,
  };
}

describe('sessions', () => {
  beforeEach(() => {
    vi.clearAllMocks();

    Object.assign(window.electron, {
      hasAcceptedRecipeBefore: mocks.hasAcceptedRecipeBefore,
      recordRecipeHash: mocks.recordRecipeHash,
    });

    mocks.createAcpSession.mockResolvedValue(makeSession());

    mocks.getConfiguredBcaipExtensions.mockResolvedValue([
      gooseExtensionEntry('developer'),
      gooseExtensionEntry('memory'),
    ]);

    mocks.beginConfiguredRecipeParameterScope.mockReturnValue({
      id: 'scope-1',
      finish: mocks.finishConfiguredRecipeParameterScope,
    });

    mocks.getAcpFeatureCapabilities.mockResolvedValue({
      localInference: false,
      recipeParameterScopes: true,
    });

    mocks.decodeRecipe.mockResolvedValue(RECIPE);

    mocks.scanRecipe.mockResolvedValue({
      has_security_warnings: false,
    });

    mocks.listSavedRecipes.mockResolvedValue([
      {
        id: 'recipe-1',
        recipe: RECIPE,
      },
    ]);

    mocks.requestRecipeConsent.mockResolvedValue(true);
    mocks.hasAcceptedRecipeBefore.mockResolvedValue(true);
    mocks.recordRecipeHash.mockResolvedValue(true);
  });

  describe('getSessionDisplayName', () => {
    it('returns the session name when there is no recipe title to use', () => {
      const session = makeSession({
        name: 'Generated title',
        user_set_name: false,
      });

      expect(getSessionDisplayName(session)).toBe('Generated title');
    });

    it('returns the user-set name before the recipe title', () => {
      const session = makeSession({
        name: 'My Renamed Chat',
        user_set_name: true,
        recipe: RECIPE,
      });

      expect(getSessionDisplayName(session)).toBe('My Renamed Chat');
    });

    it('returns the recipe title when the session was not renamed by the user', () => {
      const session = makeSession({
        name: 'auto-generated',
        user_set_name: false,
        recipe: RECIPE,
      });

      expect(getSessionDisplayName(session)).toBe('Test Recipe');
    });
  });

  describe('createSession', () => {
    it('creates a plain ACP session with no explicit extension set', async () => {
      const result = await createSession('/work');

      expect(result).toBeInstanceOf(Session);

      expect(mocks.getConfiguredBcaipExtensions).not.toHaveBeenCalled();
      expect(mocks.decodeRecipe).not.toHaveBeenCalled();
      expect(mocks.listSavedRecipes).not.toHaveBeenCalled();
      expect(mocks.requestRecipeConsent).not.toHaveBeenCalled();
      expect(mocks.beginConfiguredRecipeParameterScope).not.toHaveBeenCalled();

      expect(mocks.createAcpSession).toHaveBeenCalledOnce();
      expect(mocks.createAcpSession).toHaveBeenCalledWith('/work', undefined, {
        recipeId: undefined,
        recipeDeeplink: undefined,
        recipeParameterScopeId: undefined,
      });
    });

    it('resolves explicitly selected extensions before creating the ACP session', async () => {
      await createSession('/work', {
        extensionConfigs: [extensionConfig('developer')],
      });

      expect(mocks.getConfiguredBcaipExtensions).toHaveBeenCalledOnce();

      expect(mocks.createAcpSession).toHaveBeenCalledWith('/work', [gooseExtension('developer')], {
        recipeId: undefined,
        recipeDeeplink: undefined,
        recipeParameterScopeId: undefined,
      });
    });

    it('preserves an explicitly empty extension selection', async () => {
      await createSession('/work', {
        extensionConfigs: [],
        allExtensions: [
          configuredExtension('developer', true),
          configuredExtension('memory', false),
        ],
      });

      expect(mocks.getConfiguredBcaipExtensions).not.toHaveBeenCalled();

      expect(mocks.createAcpSession).toHaveBeenCalledWith('/work', [], {
        recipeId: undefined,
        recipeDeeplink: undefined,
        recipeParameterScopeId: undefined,
      });
    });

    it('uses only enabled configured extensions', async () => {
      await createSession('/work', {
        allExtensions: [
          configuredExtension('developer', true),
          configuredExtension('memory', false),
        ],
      });

      expect(mocks.getConfiguredBcaipExtensions).toHaveBeenCalledOnce();

      expect(mocks.createAcpSession).toHaveBeenCalledWith('/work', [gooseExtension('developer')], {
        recipeId: undefined,
        recipeDeeplink: undefined,
        recipeParameterScopeId: undefined,
      });
    });

    it('leaves extensions unspecified when no configured extension is enabled', async () => {
      await createSession('/work', {
        allExtensions: [
          configuredExtension('developer', false),
          configuredExtension('memory', false),
        ],
      });

      expect(mocks.getConfiguredBcaipExtensions).not.toHaveBeenCalled();

      expect(mocks.createAcpSession).toHaveBeenCalledWith('/work', undefined, {
        recipeId: undefined,
        recipeDeeplink: undefined,
        recipeParameterScopeId: undefined,
      });
    });

    it('leaves extensions unspecified while the configured list is empty', async () => {
      await createSession('/work', {
        allExtensions: [],
      });

      expect(mocks.getConfiguredBcaipExtensions).not.toHaveBeenCalled();

      expect(mocks.createAcpSession).toHaveBeenCalledWith('/work', undefined, {
        recipeId: undefined,
        recipeDeeplink: undefined,
        recipeParameterScopeId: undefined,
      });
    });

    it('resolves a recipe deeplink before creating its session', async () => {
      await createSession('/work', {
        recipeDeeplink: 'ENCODED_RECIPE',
      });

      expect(mocks.decodeRecipe).toHaveBeenCalledOnce();
      expect(mocks.decodeRecipe).toHaveBeenCalledWith('ENCODED_RECIPE');

      expect(mocks.hasAcceptedRecipeBefore).toHaveBeenCalledWith(RECIPE);

      expect(mocks.requestRecipeConsent).not.toHaveBeenCalled();

      expect(mocks.beginConfiguredRecipeParameterScope).toHaveBeenCalledOnce();

      expect(mocks.getAcpFeatureCapabilities).toHaveBeenCalledOnce();

      expect(mocks.createAcpSession).toHaveBeenCalledWith('/work', undefined, {
        recipeId: undefined,
        recipeDeeplink: 'ENCODED_RECIPE',
        recipeParameterScopeId: 'scope-1',
      });

      expect(mocks.finishConfiguredRecipeParameterScope).toHaveBeenCalledOnce();
    });

    it('asks for consent before creating an untrusted recipe session', async () => {
      mocks.hasAcceptedRecipeBefore.mockResolvedValue(false);

      await createSession('/work', {
        recipeDeeplink: 'ENCODED_RECIPE',
      });

      expect(mocks.scanRecipe).toHaveBeenCalledWith(RECIPE);

      expect(mocks.requestRecipeConsent).toHaveBeenCalledWith({
        recipe: RECIPE,
        hasSecurityWarnings: false,
      });

      expect(mocks.recordRecipeHash).toHaveBeenCalledWith(RECIPE);

      expect(mocks.requestRecipeConsent.mock.invocationCallOrder[0]).toBeLessThan(
        mocks.createAcpSession.mock.invocationCallOrder[0]
      );
    });

    it('does not create a session when recipe consent is declined', async () => {
      mocks.hasAcceptedRecipeBefore.mockResolvedValue(false);
      mocks.requestRecipeConsent.mockResolvedValue(false);

      await expect(
        createSession('/work', {
          recipeDeeplink: 'ENCODED_RECIPE',
        })
      ).rejects.toThrow('Recipe was not trusted by the user');

      expect(mocks.recordRecipeHash).not.toHaveBeenCalled();
      expect(mocks.createAcpSession).not.toHaveBeenCalled();
    });

    it('resolves a saved recipe by id before creating its session', async () => {
      mocks.hasAcceptedRecipeBefore.mockResolvedValue(false);
      mocks.scanRecipe.mockResolvedValue({
        has_security_warnings: true,
      });

      await createSession('/work', {
        recipeId: 'recipe-1',
      });

      expect(mocks.listSavedRecipes).toHaveBeenCalledOnce();
      expect(mocks.decodeRecipe).not.toHaveBeenCalled();

      expect(mocks.requestRecipeConsent).toHaveBeenCalledWith({
        recipe: RECIPE,
        hasSecurityWarnings: true,
      });

      expect(mocks.createAcpSession).toHaveBeenCalledWith('/work', undefined, {
        recipeId: 'recipe-1',
        recipeDeeplink: undefined,
        recipeParameterScopeId: undefined,
      });

      expect(mocks.beginConfiguredRecipeParameterScope).not.toHaveBeenCalled();
    });

    it('fails before ACP session creation when a saved recipe cannot be found', async () => {
      mocks.listSavedRecipes.mockResolvedValue([]);

      await expect(
        createSession('/work', {
          recipeId: 'missing',
        })
      ).rejects.toThrow('Recipe missing was not found in the recipe library');

      expect(mocks.createAcpSession).not.toHaveBeenCalled();
    });

    it('rejects scoped deeplink parameters when the ACP server does not support them', async () => {
      mocks.getAcpFeatureCapabilities.mockResolvedValue({
        localInference: false,
        recipeParameterScopes: false,
      });

      await expect(
        createSession('/work', {
          recipeDeeplink: 'ENCODED_RECIPE',
        })
      ).rejects.toThrow(
        'The connected Goose server does not support securely scoped deeplink recipe parameters. Update the server and try again.'
      );

      expect(mocks.createAcpSession).not.toHaveBeenCalled();

      expect(mocks.finishConfiguredRecipeParameterScope).toHaveBeenCalledOnce();
    });

    it('finishes the recipe parameter scope when ACP session creation fails', async () => {
      mocks.createAcpSession.mockRejectedValue(new Error('session creation failed'));

      await expect(
        createSession('/work', {
          recipeDeeplink: 'ENCODED_RECIPE',
        })
      ).rejects.toThrow('session creation failed');

      expect(mocks.finishConfiguredRecipeParameterScope).toHaveBeenCalledOnce();
    });

    it('finishes the recipe parameter scope when extension resolution fails', async () => {
      mocks.getConfiguredBcaipExtensions.mockRejectedValue(new Error('extension lookup failed'));

      await expect(
        createSession('/work', {
          recipeDeeplink: 'ENCODED_RECIPE',
          extensionConfigs: [extensionConfig('developer')],
        })
      ).rejects.toThrow('extension lookup failed');

      expect(mocks.createAcpSession).not.toHaveBeenCalled();

      expect(mocks.finishConfiguredRecipeParameterScope).toHaveBeenCalledOnce();
    });
  });

  describe('startNewSession', () => {
    it('creates the session, dispatches its events, navigates, and returns it', async () => {
      const session = makeSession({
        id: 'created-session',
      });

      mocks.createAcpSession.mockResolvedValue(session);

      const setView: Parameters<typeof startNewSession>[1] = vi.fn();

      const sessionCreatedListener = vi.fn();
      const activeSessionListener = vi.fn();

      window.addEventListener(AppEvents.SESSION_CREATED, sessionCreatedListener);

      window.addEventListener(AppEvents.ADD_ACTIVE_SESSION, activeSessionListener);

      try {
        const result = await startNewSession('Hello', setView, '/work');

        expect(result).toBe(session);

        expect(mocks.createAcpSession).toHaveBeenCalledWith('/work', undefined, {
          recipeId: undefined,
          recipeDeeplink: undefined,
          recipeParameterScopeId: undefined,
        });

        expect(sessionCreatedListener).toHaveBeenCalledOnce();

        const sessionCreatedEvent = sessionCreatedListener.mock.calls[0][0];

        expect(sessionCreatedEvent).toBeInstanceOf(CustomEvent);
        expect((sessionCreatedEvent as CustomEvent).detail).toEqual({
          session,
        });

        expect(activeSessionListener).toHaveBeenCalledOnce();

        const activeSessionEvent = activeSessionListener.mock.calls[0][0];

        expect(activeSessionEvent).toBeInstanceOf(CustomEvent);
        expect((activeSessionEvent as CustomEvent).detail).toEqual({
          sessionId: 'created-session',
          initialMessage: {
            msg: 'Hello',
            images: [],
          },
        });

        expect(setView).toHaveBeenCalledOnce();
        expect(setView).toHaveBeenCalledWith('pair', {
          disableAnimation: true,
          initialMessage: {
            msg: 'Hello',
            images: [],
          },
          resumeSessionId: 'created-session',
        });
      } finally {
        window.removeEventListener(AppEvents.SESSION_CREATED, sessionCreatedListener);

        window.removeEventListener(AppEvents.ADD_ACTIVE_SESSION, activeSessionListener);
      }
    });

    it('creates a session without an initial message when no text is provided', async () => {
      const session = makeSession({
        id: 'empty-session',
      });

      mocks.createAcpSession.mockResolvedValue(session);

      const setView: Parameters<typeof startNewSession>[1] = vi.fn();

      const activeSessionListener = vi.fn();

      window.addEventListener(AppEvents.ADD_ACTIVE_SESSION, activeSessionListener);

      try {
        await startNewSession(undefined, setView, '/work');

        expect(activeSessionListener).toHaveBeenCalledOnce();

        const event = activeSessionListener.mock.calls[0][0];

        expect((event as CustomEvent).detail).toEqual({
          sessionId: 'empty-session',
          initialMessage: undefined,
        });

        expect(setView).toHaveBeenCalledWith('pair', {
          disableAnimation: true,
          initialMessage: undefined,
          resumeSessionId: 'empty-session',
        });
      } finally {
        window.removeEventListener(AppEvents.ADD_ACTIVE_SESSION, activeSessionListener);
      }
    });

    it('does not dispatch or navigate when session creation fails', async () => {
      mocks.createAcpSession.mockRejectedValue(new Error('session creation failed'));

      const setView: Parameters<typeof startNewSession>[1] = vi.fn();

      const sessionCreatedListener = vi.fn();
      const activeSessionListener = vi.fn();

      window.addEventListener(AppEvents.SESSION_CREATED, sessionCreatedListener);

      window.addEventListener(AppEvents.ADD_ACTIVE_SESSION, activeSessionListener);

      try {
        await expect(startNewSession('Hello', setView, '/work')).rejects.toThrow(
          'session creation failed'
        );

        expect(sessionCreatedListener).not.toHaveBeenCalled();
        expect(activeSessionListener).not.toHaveBeenCalled();
        expect(setView).not.toHaveBeenCalled();
      } finally {
        window.removeEventListener(AppEvents.SESSION_CREATED, sessionCreatedListener);

        window.removeEventListener(AppEvents.ADD_ACTIVE_SESSION, activeSessionListener);
      }
    });
  });
});

it('rejects a numeric created_at value at runtime', () => {
  const data = {
    id: 'invalid-session',
    name: 'untitled',
    message_count: 0,
    created_at: 1234567890,
    updated_at: '2026-01-01T00:00:00.000Z',
    working_dir: '/tmp',
    extension_data: {
      active: [],
      installed: [],
    },
  };

  expect(() => Session.fromData(data as unknown as SessionData)).toThrow(
    'Date value must be a string'
  );
});

it('rejects a numeric updated_at value at runtime', () => {
  const data = {
    id: 'invalid-session',
    name: 'untitled',
    message_count: 0,
    created_at: '2026-01-01T00:00:00.000Z',
    updated_at: 1234567890,
    working_dir: '/tmp',
    extension_data: {
      active: [],
      installed: [],
    },
  };

  expect(() => Session.fromData(data as unknown as SessionData)).toThrow(
    'Date value must be a string'
  );
});
