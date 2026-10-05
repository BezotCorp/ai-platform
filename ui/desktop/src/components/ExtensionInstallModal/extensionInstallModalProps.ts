import type { ExtensionConfig } from '../../types/extensionConfig';
import { View, ViewOptions } from '../../utils/navigationUtils';

export interface ExtensionInstallModalProps {
  addExtension?: (name: string, config: ExtensionConfig, enabled: boolean) => Promise<void>;
  setView: (view: View, options?: ViewOptions) => void;
}
