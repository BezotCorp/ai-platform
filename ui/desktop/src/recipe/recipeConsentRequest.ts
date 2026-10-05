import type { Recipe } from './recipe';

export interface RecipeConsentRequest {
  id: string;
  recipe: Recipe;
  hasSecurityWarnings: boolean;
}
