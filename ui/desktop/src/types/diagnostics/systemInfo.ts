export type SystemInfo = {
  app_version: string;
  architecture: string;
  enabled_extensions: string[];
  model?: string | null;
  os: string;
  os_version: string;
  provider?: string | null;
};
