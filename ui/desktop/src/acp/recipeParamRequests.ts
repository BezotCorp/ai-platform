import type {
  RecipeParamsResponseUnstable,
  RequestRecipeParamsUnstable,
} from '@aaif/goose-acp-client';
import { v7 as uuidv7 } from 'uuid';

import type { AcpRecipeParamRequest } from './acpRecipeParamRequest';
import type { PendingRecipeParamRequest } from './pendingRecipeParamRequest';
import type { ConfiguredParameterState } from './configuredParameterState';
import type { ConfiguredRecipeParameterScope } from './configuredRecipeParameterScope';
const pendingRequests = new Map<string, PendingRecipeParamRequest>();
const listeners = new Set<() => void>();
let snapshot: AcpRecipeParamRequest[] = [];
let configuredParameterState: ConfiguredParameterState = { status: 'uninitialized' };

function emit(): void {
  snapshot = Array.from(pendingRequests.values(), (pending) => pending.request);
  for (const listener of listeners) {
    listener();
  }
}

export function subscribeAcpRecipeParamRequests(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

export function getAcpRecipeParamRequestsSnapshot(): AcpRecipeParamRequest[] {
  return snapshot;
}

function consumeConfiguredParameters(): boolean {
  if (configuredParameterState.status !== 'active') {
    return false;
  }

  const sessionId = configuredParameterState.sessionId;
  configuredParameterState = { status: 'consumed' };
  let scrubbedPendingRequest = false;
  if (sessionId) {
    for (const pending of pendingRequests.values()) {
      if (pending.usesConfiguredParameters && pending.request.sessionId === sessionId) {
        pending.request.initialValues = {};
        scrubbedPendingRequest = true;
      }
    }
  }
  return scrubbedPendingRequest;
}

export function beginConfiguredRecipeParameterScope(): ConfiguredRecipeParameterScope | undefined {
  if (configuredParameterState.status !== 'uninitialized') {
    return undefined;
  }

  const configured = window.appConfig?.get('recipeParameters') as
    Record<string, string> | undefined;
  if (!configured || Object.keys(configured).length === 0) {
    configuredParameterState = { status: 'consumed' };
    return undefined;
  }

  const scopeId = `configured_recipe_parameters_${uuidv7()}`;
  configuredParameterState = {
    status: 'active',
    scopeId,
    values: { ...configured },
  };
  return {
    id: scopeId,
    finish: () => {
      if (
        configuredParameterState.status === 'active' &&
        configuredParameterState.scopeId === scopeId &&
        consumeConfiguredParameters()
      ) {
        emit();
      }
    },
  };
}

function configuredParameterValues(request: RequestRecipeParamsUnstable): {
  values: Record<string, string>;
  usesConfiguredParameters: boolean;
} {
  if (
    configuredParameterState.status !== 'active' ||
    request.parameterScopeId !== configuredParameterState.scopeId
  ) {
    return { values: {}, usesConfiguredParameters: false };
  }
  configuredParameterState.sessionId ??= request.sessionId;
  if (configuredParameterState.sessionId !== request.sessionId) {
    return { values: {}, usesConfiguredParameters: false };
  }
  const fileParameterKeys = new Set(
    request.parameters
      .filter((parameter) => parameter.input_type === 'file')
      .map((parameter) => parameter.key)
  );
  return {
    values: Object.fromEntries(
      Object.entries(configuredParameterState.values).filter(([key]) => !fileParameterKeys.has(key))
    ),
    usesConfiguredParameters: true,
  };
}

export async function requestAcpRecipeParams(
  request: RequestRecipeParamsUnstable
): Promise<RecipeParamsResponseUnstable> {
  const { values: initialValues, usesConfiguredParameters } = configuredParameterValues(request);
  const paramRequest: AcpRecipeParamRequest = {
    id: `acp_recipe_params_${uuidv7()}`,
    sessionId: request.sessionId,
    parameters: request.parameters,
    initialValues,
  };

  return new Promise<RecipeParamsResponseUnstable>((resolve) => {
    pendingRequests.set(paramRequest.id, {
      request: paramRequest,
      resolve,
      usesConfiguredParameters,
    });
    emit();
  });
}

export function resolveAcpRecipeParamRequest(id: string, values: Record<string, string>): boolean {
  const pending = pendingRequests.get(id);
  if (!pending) {
    return false;
  }
  pendingRequests.delete(id);
  if (pending.usesConfiguredParameters) {
    consumeConfiguredParameters();
  }
  emit();
  pending.resolve({ action: 'submit', values });
  return true;
}

export function cancelAcpRecipeParamRequest(id: string): void {
  const pending = pendingRequests.get(id);
  if (!pending) {
    return;
  }
  pendingRequests.delete(id);
  if (pending.usesConfiguredParameters) {
    consumeConfiguredParameters();
  }
  emit();
  pending.resolve({ action: 'cancel' });
}
