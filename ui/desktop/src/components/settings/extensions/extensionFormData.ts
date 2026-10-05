export interface ExtensionFormData {
  name: string;
  description: string;
  type: 'stdio' | 'streamable_http' | 'builtin';
  cmd?: string;
  endpoint?: string;
  enabled: boolean;
  timeout?: number;
  envVars: {
    key: string;
    value: string;
    isEdited?: boolean;
  }[];
  headers: {
    key: string;
    value: string;
    isEdited?: boolean;
  }[];
  installation_notes?: string;
  available_tools?: string[];
  // streamable_http fields with no form input yet; carried through so an
  // unrelated edit does not strip them from the saved config.
  socket?: string | null;
  client_id?: string | null;
  client_secret_key?: string | null;
  scopes?: string[];
}
