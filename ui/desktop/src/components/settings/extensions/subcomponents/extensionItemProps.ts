import type { ConfiguredExtensionEntry } from '../../../../types/configuredExtensionEntry';

export interface ExtensionItemProps {
  extension: ConfiguredExtensionEntry;
  onToggle: (extension: ConfiguredExtensionEntry) => Promise<boolean | void> | void;
  onConfigure?: (extension: ConfiguredExtensionEntry) => void;
  isStatic?: boolean; // to not allow users to edit configuration
}
