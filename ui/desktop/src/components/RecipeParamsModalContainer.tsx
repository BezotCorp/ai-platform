import React, { useSyncExternalStore } from 'react';
import {
  cancelAcpRecipeParamRequest,
  getAcpRecipeParamRequestsSnapshot,
  resolveAcpRecipeParamRequest,
  subscribeAcpRecipeParamRequests,
} from '../acp/recipeParamRequests';
import ParameterInputModal from './ParameterInputModal';

export default function RecipeParamsModalContainer(): React.ReactElement | null {
  const requests = useSyncExternalStore(
    subscribeAcpRecipeParamRequests,
    getAcpRecipeParamRequestsSnapshot
  );
  const request = requests[0];
  if (!request) {
    return null;
  }

  return (
    <ParameterInputModal
      key={request.id}
      parameters={request.parameters}
      initialValues={request.initialValues}
      onSubmit={(values) => resolveAcpRecipeParamRequest(request.id, values)}
      onClose={() => cancelAcpRecipeParamRequest(request.id)}
    />
  );
}
