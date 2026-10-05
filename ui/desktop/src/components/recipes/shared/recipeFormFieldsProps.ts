import { type RecipeFormApi } from './recipeFormSchema';

export interface RecipeFormFieldsProps {
  // Form instance from parent
  form: RecipeFormApi;

  // Event handlers
  onTitleChange?: (value: string) => void;
  onDescriptionChange?: (value: string) => void;
  onInstructionsChange?: (value: string) => void;
  onPromptChange?: (value: string) => void;
  onJsonSchemaChange?: (value: string) => void;
}
