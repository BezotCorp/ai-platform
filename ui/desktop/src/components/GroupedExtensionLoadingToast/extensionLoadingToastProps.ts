import type { ExtensionLoadingStatus } from './extensionLoadingStatus';

export interface ExtensionLoadingToastProps {
  extensions: ExtensionLoadingStatus[];
  totalCount: number;
  isComplete: boolean;
}
