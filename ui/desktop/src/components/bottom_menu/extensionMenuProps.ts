import type { ExtensionMenuEntry } from './extensionMenuEntry';

export interface ExtensionMenuProps {
  extensions: ExtensionMenuEntry[];
  title: string;
  searchPlaceholder: string;
  description: string;
  emptyMessage: string;
  noResultsMessage: string;
  hidden: boolean;
  isTransitioning: boolean;
  isSortPending: boolean;
  togglingExtensionName: string | null;
  onToggle: (extension: ExtensionMenuEntry) => void;
  onClose?: () => void;
}
