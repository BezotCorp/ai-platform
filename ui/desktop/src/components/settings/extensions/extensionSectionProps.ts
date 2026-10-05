import type { ExtensionConfig } from '../../../types/extensionConfig';
import type { ConfiguredExtensionEntry } from '../../../types/configuredExtensionEntry';

export interface ExtensionSectionProps {
  deepLinkConfig?: ExtensionConfig;
  showEnvVars?: boolean;
  hideButtons?: boolean;
  disableConfiguration?: boolean;
  customToggle?: (extension: ConfiguredExtensionEntry) => Promise<boolean | void>;
  selectedExtensions?: string[]; // Add controlled state
  onModalClose?: (extensionName: string) => void;
  searchTerm?: string;
}
