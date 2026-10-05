import type { RecipeParamsResponseUnstable } from '@aaif/goose-acp-client';
import type { AcpRecipeParamRequest } from './acpRecipeParamRequest';

export interface PendingRecipeParamRequest {
  request: AcpRecipeParamRequest;
  resolve: (response: RecipeParamsResponseUnstable) => void;
  usesConfiguredParameters: boolean;
}
