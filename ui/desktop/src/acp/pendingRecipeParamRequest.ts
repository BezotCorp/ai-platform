import type { RecipeParamsResponseUnstable } from '@bezotcorp/bcaip-acp-client';
import type { AcpRecipeParamRequest } from './acpRecipeParamRequest';

export interface PendingRecipeParamRequest {
  request: AcpRecipeParamRequest;
  resolve: (response: RecipeParamsResponseUnstable) => void;
  usesConfiguredParameters: boolean;
}
