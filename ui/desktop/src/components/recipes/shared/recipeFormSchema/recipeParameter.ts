import { z } from 'zod';
import { parameterSchema } from './parameterSchema';

export type RecipeParameter =
  z.infer<typeof parameterSchema>;
