import type { JsonSchemaProperty } from './jsonSchemaProperty';

export interface JsonSchema {
  type?: string;
  properties?: Record<string, JsonSchemaProperty>;
  required?: string[];
  title?: string;
  description?: string;
}
