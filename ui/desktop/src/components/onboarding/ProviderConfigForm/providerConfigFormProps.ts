import type { ProviderDetails } from '../../../types/providerDetails';
import type { OnConfigured } from '../ProviderConfigForm/onConfigured';

export interface ProviderConfigFormProps {
  provider: ProviderDetails;
  onConfigured: OnConfigured;
}
