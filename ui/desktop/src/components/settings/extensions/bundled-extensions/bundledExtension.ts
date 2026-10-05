export type BundledExtension = {
  id: string;
  name: string;
  display_name?: string;
  description: string;
  enabled: boolean;
  type: 'builtin' | 'stdio' | 'streamable_http';
  cmd?: string;
  args?: string[];
  uri?: string;
  envs?: { [key: string]: string };
  env_keys?: Array<string>;
  timeout?: number;
  allow_configure?: boolean;
};
