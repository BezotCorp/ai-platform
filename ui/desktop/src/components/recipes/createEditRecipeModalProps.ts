import type { Recipe } from '../../recipe';

export interface CreateEditRecipeModalProps {
  isOpen: boolean;
  onClose: (wasSaved?: boolean) => void;
  recipe?: Recipe;
  isCreateMode?: boolean;
  recipeId?: string | null;
  onRecipeSaved?: (savedRecipeId: string) => void;
}
