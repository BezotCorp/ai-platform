import { KeyboardShortcuts } from '../../../utils/keyboardShortcuts';

export interface ShortcutRecorderProps {
  value: string;
  onSave: (shortcut: string) => void;
  onCancel: () => void;
  allShortcuts?: KeyboardShortcuts;
  currentKey?: keyof KeyboardShortcuts;
}
