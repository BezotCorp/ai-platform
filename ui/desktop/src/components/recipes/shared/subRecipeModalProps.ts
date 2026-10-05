import { SubRecipeFormData } from './recipeFormSchema';

export interface SubRecipeModalProps {
  isOpen: boolean;
  onClose: () => void;
  onSave: (subRecipe: SubRecipeFormData) => boolean;
  subRecipe?: SubRecipeFormData | null;
}
