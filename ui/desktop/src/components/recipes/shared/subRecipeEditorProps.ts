import { SubRecipeFormData } from './recipeFormSchema';

export interface SubRecipeEditorProps {
  subRecipes: SubRecipeFormData[];
  onChange: (subRecipes: SubRecipeFormData[]) => void;
}
