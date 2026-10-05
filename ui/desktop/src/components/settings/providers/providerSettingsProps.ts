export interface ProviderSettingsProps {
  onClose: () => void;
  isOnboarding: boolean;
  onProviderLaunched?: (model?: string) => void;
}
