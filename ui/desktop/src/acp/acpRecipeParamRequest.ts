import type { RecipeParameterDto } from '@bezotcorp/bcaip-acp-client';

export interface AcpRecipeParamRequest {
  id: string;
  sessionId: string;
  parameters: RecipeParameterDto[];
  initialValues?: Record<string, string>;
}
