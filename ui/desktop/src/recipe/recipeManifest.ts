import type { RecipeListEntryDto } from '@aaif/goose-acp-client';
import type { Recipe } from './recipe';

export type RecipeManifest = Omit<RecipeListEntryDto, 'recipe'> & {
  recipe: Recipe;
};
