import type { ProviderDetails } from '../../../../types/providerDetails';

export interface ProviderConfigurationModalProps {
  provider: ProviderDetails;
  onClose: () => void;
  onConfigured?: (provider: ProviderDetails) => void;
}
