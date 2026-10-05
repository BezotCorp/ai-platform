import { z } from 'zod';
import { recipeFormSchema } from './recipeFormSchema';

export type RecipeFormData =
  z.infer<typeof recipeFormSchema>;
