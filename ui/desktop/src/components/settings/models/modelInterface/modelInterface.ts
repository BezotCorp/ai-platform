import type { Model } from './model';
export type { Model } from './model';

import { listLocalModels } from '../../../../acp/local-inference';
import { acpGetProviderDetails, acpListProviderModels } from '../../../../acp/providers';
import type { ProviderDetails } from '../../../../types/providerDetails';
import type { ThinkingEffort } from '../../../../types/thinkingEffort';
import { errorMessage as getErrorMessage } from '../../../../utils/conversionUtils';

import type { ProviderModelsResult } from '../modelInterface/providerModelsResult';

export async function getProviderMetadata(providerName: string) {
  return (await acpGetProviderDetails(providerName)).metadata;
}


export async function fetchModelsForProviders(
  activeProviders: ProviderDetails[]
): Promise<ProviderModelsResult[]> {
  const modelPromises = activeProviders.map(async (p) => {
    try {
      // For local provider, use listLocalModels and filter to only downloaded models
      if (p.name === 'local') {
        const allModels = await listLocalModels();
        const downloadedModels = allModels
          .filter((m) => m.status.state === 'Downloaded')
          .map((m) => ({ name: m.id, provider: p.name }));
        return { provider: p, models: downloadedModels, error: null, warning: null };
      }

      const providerModels = await acpListProviderModels(p.name);
      const models = providerModels.map((m) => ({
        name: m.id,
        provider: p.name,
        context_limit: m.contextLimit ?? undefined,
        reasoning: m.reasoning ?? undefined,
      }));
      return { provider: p, models, error: null, warning: null };
    } catch (e: unknown) {
      // For custom providers, fall back to the configured model list
      if (p.provider_type === 'Custom') {
        const fallbackModels = p.metadata.known_models.map(
          (m) =>
            ({
              name: m.name,
              provider: p.name,
              context_limit: m.context_limit,
              reasoning: m.reasoning ?? undefined,
            }) as Model
        );
        if (fallbackModels.length > 0) {
          console.warn(`Failed to fetch models for ${p.name}:`, getErrorMessage(e));
          return {
            provider: p,
            models: fallbackModels,
            error: null,
            warning: `Could not fetch models from provider — showing configured models instead.`,
          };
        }
      }

      const errMsg = getErrorMessage(e);
      const errorMessage = `Failed to fetch models for ${p.name}${errMsg ? `: ${errMsg}` : ''}`;
      return {
        provider: p,
        models: null,
        error: errorMessage,
        warning: null,
      };
    }
  });

  return await Promise.all(modelPromises);
}

export async function fetchModelReasoning(
  provider: string,
  model: string,
  fallback?: boolean
): Promise<boolean | null> {
  try {
    const models = await acpListProviderModels(provider);
    const match = models.find((m) => m.id === model);
    return match?.reasoning ?? fallback ?? null;
  } catch {
    return fallback ?? null;
  }
}

export type { ProviderModelsResult } from '../modelInterface/providerModelsResult';
