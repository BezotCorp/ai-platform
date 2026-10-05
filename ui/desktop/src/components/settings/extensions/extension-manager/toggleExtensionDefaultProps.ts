import type { ExtensionConfig } from '../../../../types/extensionConfig';

export interface ToggleExtensionDefaultProps {
  toggle: 'toggleOn' | 'toggleOff';
  extensionConfig: ExtensionConfig;
  setEnabled: (enabled: boolean) => Promise<void>;
}
