import {
  deleteRecipe as acpDeleteRecipe,
  listRecipes as acpListRecipes,
  recipeToYaml as acpRecipeToYaml,
  saveRecipe as acpSaveRecipe,
  scheduleRecipe as acpScheduleRecipe,
  setRecipeSlashCommand as acpSetRecipeSlashCommand,
} from '../acp/recipe';
import { stripEmptyExtensions } from '.';
import type { Recipe, RecipeManifest } from '.';
import { AppDate } from '../utils/appDate';

export const saveRecipe = async (
  recipe: Recipe,
  recipeId?: string | null
): Promise<{ id: string; fileName: string; filePath: string }> => {
  try {
    const response = await acpSaveRecipe(stripEmptyExtensions(recipe), recipeId);
    return {
      id: response.id,
      fileName: response.file_name,
      filePath: response.file_path,
    };
  } catch (error) {
    let error_message = 'unknown error';
    if (typeof error === 'object' && error !== null && 'message' in error) {
      error_message = error.message as string;
    }
    throw new Error(error_message, { cause: error });
  }
};

export const listSavedRecipes = async (): Promise<RecipeManifest[]> => {
  return await acpListRecipes();
};

export const deleteRecipe = async (id: string): Promise<void> => {
  await acpDeleteRecipe(id);
};

export const scheduleRecipe = async (id: string, cronSchedule?: string | null): Promise<void> => {
  await acpScheduleRecipe(id, cronSchedule);
};

export const setRecipeSlashCommand = async (
  id: string,
  slashCommand?: string | null
): Promise<void> => {
  await acpSetRecipeSlashCommand(id, slashCommand);
};

export const recipeToYaml = async (recipe: Recipe): Promise<string> => {
  return await acpRecipeToYaml(recipe);
};

export const convertToLocaleDateString = (lastModified: string): string => {
  if (!lastModified) {
    return '';
  }

  return AppDate.fromString(lastModified).toLocaleDateString();
};

export const getStorageDirectory = (isGlobal: boolean): string => {
  if (isGlobal) {
    const pathRoot = window.appConfig.get('BCAIP_PATH_ROOT') as string | undefined;
    if (pathRoot) {
      return `${pathRoot}/config/recipes`;
    }
    const configDir = window.appConfig.get('BCAIP_CONFIG_DIR') as string | undefined;
    if (configDir) {
      return `${configDir}/recipes`;
    }
    return '~/.config/bcaip/recipes';
  } else {
    // For directory recipes, build absolute path using working directory
    const workingDir = window.appConfig.get('BCAIP_WORKING_DIR') as string;
    return `${workingDir}/.bcaip/recipes`;
  }
};
