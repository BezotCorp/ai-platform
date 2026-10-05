import type { ConfiguredExtensionEntry } from '../../../../types/configuredExtensionEntry';

export interface ExtensionListProps {
  extensions: ConfiguredExtensionEntry[];
  onToggle: (extension: ConfiguredExtensionEntry) => Promise<boolean | void> | void;
  onConfigure?: (extension: ConfiguredExtensionEntry) => void;
  isStatic?: boolean;
  disableConfiguration?: boolean;
  searchTerm?: string;
}
