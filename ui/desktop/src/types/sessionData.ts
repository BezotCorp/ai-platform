import type { Recipe } from '../recipe';
import type { ExtensionData, GooseMode, Message, ModelConfig, SessionType, Usage } from '.';

/**
 * Serializable data used to construct and copy a `Session`.
 *
 * `SessionData` contains session state only. Session behavior belongs to the
 * `Session` class.
 */
export interface SessionData {
  accumulated_cost?: number | null;
  accumulated_usage?: Usage;
  archived_at?: string | null;
  conversation?: Message[] | null;
  created_at: string;
  extension_data: ExtensionData;
  goose_mode?: GooseMode;
  id: string;
  last_message_at?: string | null;
  last_message_snippet?: string | null;
  message_count: number;
  model_config?: ModelConfig | null;
  name: string;
  project_id?: string | null;
  provider_name?: string | null;
  recipe?: Recipe | null;
  schedule_id?: string | null;
  session_type?: SessionType;
  updated_at: string;
  usage?: Usage;
  user_recipe_values?: Record<string, string> | null;
  user_set_name?: boolean;
  working_dir: string;
}
