import { type MessageDescriptor } from 'react-intl';
import { KeyboardShortcuts } from '../../../utils/keyboardShortcuts';

export interface ShortcutConfig {
  key: keyof KeyboardShortcuts;
  label: MessageDescriptor;
  description: MessageDescriptor;
  category: 'global' | 'application' | 'search' | 'window';
}
