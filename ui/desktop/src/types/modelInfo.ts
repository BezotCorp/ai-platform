export type ModelInfo = {
  context_limit?: number | null;
  currency?: string | null;
  input_token_cost?: number | null;
  name: string;
  output_token_cost?: number | null;
  reasoning?: boolean;
  resolved_model?: string | null;
  supports_cache_control?: boolean | null;
};
