import type { KeyboardShortcuts } from './keyboardShortcuts';

export type DefaultKeyboardShortcuts = {
  [K in keyof KeyboardShortcuts]: string;
};
