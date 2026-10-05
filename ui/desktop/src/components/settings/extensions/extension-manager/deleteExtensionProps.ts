import type { ExtensionConfig } from '../../../../types/extensionConfig';

export interface DeleteExtensionProps {
  name: string;
  removeFromConfig: (name: string) => Promise<void>;
  extensionConfig?: ExtensionConfig;
}
