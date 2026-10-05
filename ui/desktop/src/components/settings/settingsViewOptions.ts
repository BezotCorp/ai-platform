import type { ExtensionConfig } from '../../types/extensionConfig';

export type SettingsViewOptions = {
  deepLinkConfig?: ExtensionConfig;
  showEnvVars?: boolean;
  section?: string;
};
