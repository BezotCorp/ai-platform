import { z } from 'zod';

export const subRecipeSchema = z.object({
  name: z.string().min(
    1,
    'Subrecipe name is required'
  ),
  path: z.string().min(
    1,
    'Subrecipe path is required'
  ),
  description: z.string().optional(),
  values: z
    .record(z.string(), z.string())
    .nullable()
    .optional(),
  sequential_when_repeated: z
    .boolean()
    .default(false),
});
