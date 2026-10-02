import type {
  RecipeDto,
  SaveRecipeResponseUnstable,
  ScanRecipeResponseUnstable,
  RecipeListEntryDto,
} from '@aaif/goose-acp-client';
import { getAcpClient } from './acpConnection';

let inFlightListRecipes: Promise<RecipeListEntryDto[]> | null = null;

function acpErrorMessage(error: unknown): string | null {
  if (typeof error !== 'object' || error === null) {
    return null;
  }

  const candidate = 'error' in error && isRecord(error.error) ? error.error : error;
  if (!isRecord(candidate)) {
    return null;
  }
  if (typeof candidate.data === 'string') {
    return candidate.data;
  }
  return typeof candidate.message === 'string' ? candidate.message : null;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null;
}

function normalizeAcpError(error: unknown, fallback: string): Error {
  const message = acpErrorMessage(error);
  if (message) {
    return new Error(message);
  }
  if (error instanceof Error) {
    return error;
  }
  return new Error(fallback);
}

export async function encodeRecipe(recipe: RecipeDto): Promise<string> {
  try {
    const client = await getAcpClient();
    const response = await client.goose.recipesEncodeUnstable({ recipe });
    return response.deeplink;
  } catch (error) {
    throw normalizeAcpError(error, 'Failed to encode recipe');
  }
}

export async function decodeRecipe(deeplink: string): Promise<RecipeDto> {
  try {
    const client = await getAcpClient();
    const response = await client.goose.recipesDecodeUnstable({ deeplink });
    return response.recipe;
  } catch (error) {
    throw normalizeAcpError(error, 'Failed to decode recipe');
  }
}

export async function scanRecipe(recipe: RecipeDto): Promise<ScanRecipeResponseUnstable> {
  try {
    const client = await getAcpClient();
    return await client.goose.recipesScanUnstable({ recipe });
  } catch (error) {
    throw normalizeAcpError(error, 'Failed to scan recipe');
  }
}

export async function parseRecipe(content: string): Promise<RecipeDto> {
  try {
    const client = await getAcpClient();
    const response = await client.goose.recipesParseUnstable({ content });
    return response.recipe;
  } catch (error) {
    throw normalizeAcpError(error, 'Failed to parse recipe');
  }
}

export async function saveRecipe(
  recipe: RecipeDto,
  id?: string | null
): Promise<SaveRecipeResponseUnstable> {
  try {
    const client = await getAcpClient();
    return await client.goose.recipesSaveUnstable({
      recipe,
      id,
    });
  } catch (error) {
    throw normalizeAcpError(error, 'Failed to save recipe');
  }
}

export async function listRecipes(): Promise<RecipeListEntryDto[]> {
  const pending = inFlightListRecipes;
  if (pending) {
    return pending;
  }

  const listPromise = (async () => {
    const client = await getAcpClient();
    const response = await client.goose.recipesListUnstable({});
    return response.recipes;
  })().catch((error) => {
    throw normalizeAcpError(error, 'Failed to list recipes');
  });

  inFlightListRecipes = listPromise;

  try {
    return await listPromise;
  } finally {
    if (inFlightListRecipes === listPromise) {
      inFlightListRecipes = null;
    }
  }
}

export async function deleteRecipe(id: string): Promise<void> {
  try {
    const client = await getAcpClient();
    await client.goose.recipesDeleteUnstable({ id });
  } catch (error) {
    throw normalizeAcpError(error, 'Failed to delete recipe');
  }
}

export async function scheduleRecipe(id: string, cronSchedule?: string | null): Promise<void> {
  try {
    const client = await getAcpClient();
    await client.goose.recipesScheduleUnstable({ id, cron_schedule: cronSchedule });
  } catch (error) {
    throw normalizeAcpError(error, 'Failed to schedule recipe');
  }
}

export async function setRecipeSlashCommand(
  id: string,
  slashCommand?: string | null
): Promise<void> {
  try {
    const client = await getAcpClient();
    await client.goose.recipesSlashCommandUnstable({ id, slash_command: slashCommand });
  } catch (error) {
    throw normalizeAcpError(error, 'Failed to set recipe slash command');
  }
}

export async function recipeToYaml(recipe: RecipeDto): Promise<string> {
  try {
    const client = await getAcpClient();
    const response = await client.goose.recipesToYamlUnstable({ recipe });
    return response.yaml;
  } catch (error) {
    throw normalizeAcpError(error, 'Failed to convert recipe to YAML');
  }
}
