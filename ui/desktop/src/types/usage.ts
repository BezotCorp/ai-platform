/**
 * Token usage counters associated with a session.
 */
export type Usage = {
  cache_read_input_tokens?: number | null;
  cache_write_input_tokens?: number | null;
  input_tokens?: number | null;
  output_tokens?: number | null;
  total_tokens?: number | null;
};
