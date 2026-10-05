import type { JsonSchema } from '../JsonSchemaForm/jsonSchema';

export interface JsonSchemaFormProps {
  schema: JsonSchema;
  onSubmit: (data: Record<string, unknown>) => void;
  onCancel?: () => void;
  submitLabel?: string;
  cancelLabel?: string;
  disabled?: boolean;
}
