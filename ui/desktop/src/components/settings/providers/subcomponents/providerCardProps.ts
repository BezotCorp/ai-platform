import type { ProviderDetails } from '../../../../types/providerDetails';

export type ProviderCardProps = {
  provider: ProviderDetails;
  onConfigure: () => void;
  onLaunch: () => void;
  isOnboarding: boolean;
};
