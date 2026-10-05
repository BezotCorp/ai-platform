import { z } from 'zod';
import { subRecipeSchema } from './subRecipeSchema';

export type SubRecipeFormData =
  z.infer<typeof subRecipeSchema>;
