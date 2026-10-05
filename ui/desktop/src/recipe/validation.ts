import type { JsonSchema } from './jsonSchema';

import { recipeDtoSchema } from '@aaif/goose-acp-client';
import { z } from 'zod';


const recipeDescription =
  'A Recipe represents a reusable agent configuration with instructions, optional prompt, parameters, supported extensions, settings, and subrecipes.';

let recipeJsonSchema: JsonSchema | null = null;

export function getRecipeJsonSchema(): JsonSchema {
  if (!recipeJsonSchema) {
    recipeJsonSchema = {
      ...(z.toJSONSchema(recipeDtoSchema, { target: 'draft-07', reused: 'inline' }) as JsonSchema),
      title: 'Recipe',
      description: recipeDescription,
    };
  }

  return recipeJsonSchema;
}
