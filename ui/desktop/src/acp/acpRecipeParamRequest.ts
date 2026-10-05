import type { RecipeParameterDto } from '@aaif/goose-acp-client';

export interface AcpRecipeParamRequest {
  id: string;
  sessionId: string;
  parameters: RecipeParameterDto[];
  initialValues?: Record<string, string>;
}
