import type { ProviderMetadata } from './providerMetadata';
import type { ProviderType } from './providerType';

export type ProviderDetails = {
  is_configured: boolean;
  is_available: boolean;
  is_refreshing?: boolean;
  last_refresh_error?: string | null;
  supports_refresh?: boolean;
  visible_in_setup: boolean;
  deprecated: boolean;
  replacement?: string | null;
  metadata: ProviderMetadata;
  name: string;
  provider_type: ProviderType;
  uses_acp: boolean;
  saved_model?: string | null;
};
