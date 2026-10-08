import type { RecipeListEntryDto } from '@bezotcorp/bcaip-acp-client';
import type { Recipe } from './recipe';

export type RecipeManifest = Omit<RecipeListEntryDto, 'recipe'> & {
  recipe: Recipe;
};
