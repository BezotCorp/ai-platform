import type { RecipeExtension } from '../../../../recipe';

export type DisplayRecipeExtension = RecipeExtension & {
  enabled?: boolean;
};
