import type { RecipeConsentRequest } from './recipeConsentRequest';

export interface PendingConsent {
  request: RecipeConsentRequest;
  resolve: (accepted: boolean) => void;
}
