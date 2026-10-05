import type { ProviderDetails } from '../../../../types/providerDetails';
import type Model from './modelInterface';

export interface ProviderModelsResult {
  provider: ProviderDetails;
  models: Model[] | null;
  error: string | null;
  warning: string | null;
}
