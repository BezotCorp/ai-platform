import type { ConfigKey } from './configKey';
import type { ModelInfo } from './modelInfo';

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
