import type { ConfigKey, ModelInfo } from '.';

export type ProviderMetadata = {
  config_keys: ConfigKey[];
  default_model: string;
  description: string;
  display_name: string;
  known_models: ModelInfo[];
  model_doc_link: string;
  name: string;
  setup_steps?: string[];
};
