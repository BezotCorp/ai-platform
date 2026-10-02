/**
 * Model configuration associated with a session.
 */
export type ModelConfig = {
  context_limit?: number | null;
  max_tokens?: number | null;
  model_name: string;
  reasoning?: boolean | null;
  request_params?: Record<string, unknown> | null;
  temperature?: number | null;
  toolshim: boolean;
  toolshim_model?: string | null;
};
