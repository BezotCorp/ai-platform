import { z } from 'zod';
import type { RecipeExtension } from '../../../../recipe';
import { parameterSchema } from './parameterSchema';
import { subRecipeSchema } from './subRecipeSchema';

export const recipeFormSchema = z.object({
  title: z
    .string()
    .min(1, 'Title is required')
    .min(3, 'Title must be at least 3 characters')
    .max(100, 'Title must be 100 characters or less'),

  description: z
    .string()
    .min(1, 'Description is required')
    .min(10, 'Description must be at least 10 characters')
    .max(500, 'Description must be 500 characters or less'),

  instructions: z
    .string()
    .min(1, 'Instructions are required')
    .min(20, 'Instructions must be at least 20 characters'),

  prompt: z.string().optional(),

  activities: z.array(z.string()).default([]),

  parameters: z.array(parameterSchema).default([]),

  jsonSchema: z.string().optional(),

  model: z.string().optional(),

  provider: z.string().optional(),

  extensions: z
    .array(z.custom<RecipeExtension>())
    .optional(),

  subRecipes: z.array(subRecipeSchema).default([]),
});
