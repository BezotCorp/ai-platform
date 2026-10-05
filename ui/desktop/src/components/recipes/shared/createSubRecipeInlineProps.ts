import { SubRecipeFormData } from './recipeFormSchema';

export interface CreateSubRecipeInlineProps {
  isOpen: boolean;
  onClose: () => void;
  onSubRecipeSaved: (subRecipe: SubRecipeFormData) => void;
  existingSubRecipes?: SubRecipeFormData[];
}
