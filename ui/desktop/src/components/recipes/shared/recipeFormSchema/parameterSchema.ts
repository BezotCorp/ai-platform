import { z } from 'zod';

export const parameterSchema = z.object({
  key: z.string().min(1, 'Parameter key is required'),
  input_type: z.enum([
    'string',
    'number',
    'boolean',
    'date',
    'file',
    'select',
  ]),
  requirement: z.enum([
    'required',
    'optional',
    'user_prompt',
  ]),
  description: z.string().min(
    1,
    'Parameter description is required'
  ),
  default: z.string().nullable().optional(),
  options: z.array(z.string()).nullable().optional(),
});
