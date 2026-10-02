import type { ExternalBackendConfig } from './externalBackendConfig';
import type { KeyboardShortcuts } from './keyboardShortcuts';
import type { DefaultKeyboardShortcuts } from './defaultKeyboardShortcuts';
import type { RecentModel } from './RecentModel';
import { LanguageSetting } from './languageSetting';

export interface Settings {
  // Desktop app settings
  showMenuBarIcon: boolean;
  disableAutoDownload: boolean;
  showDockIcon: boolean;
  enableWakelock: boolean;
  enableNotifications: boolean;
  spellcheckEnabled: boolean;
  // Key is kept as `externalGoosed` for backward compat with persisted user settings.
  externalGoosed: ExternalBackendConfig;
  globalShortcut?: string | null;
  keyboardShortcuts: KeyboardShortcuts;

  // UI preferences (migrated from localStorage)
  theme: 'dark' | 'light' | 'aura';
  useSystemTheme: boolean;
  language: LanguageSetting;
  responseStyle: string;
  showPricing: boolean;
  seenAnnouncementIds: string[];
  recentModels: RecentModel[];
  useLegacyAgentLoop: boolean;
}

export const defaultKeyboardShortcuts: DefaultKeyboardShortcuts = {
  focusWindow: 'CommandOrControl+Alt+G',
  quickLauncher: 'CommandOrControl+Alt+Shift+G',
  newChat: 'CommandOrControl+T',
  newChatWindow: 'CommandOrControl+N',
  openDirectory: 'CommandOrControl+O',
  settings: 'CommandOrControl+,',
  find: 'CommandOrControl+F',
  findNext: 'CommandOrControl+G',
  findPrevious: 'CommandOrControl+Shift+G',
  alwaysOnTop: 'CommandOrControl+Shift+T',
  toggleNavigation: 'CommandOrControl+/',
};

export const defaultSettings: Settings = {
  // Desktop app settings
  showMenuBarIcon: true,
  disableAutoDownload: false,
  showDockIcon: true,
  enableWakelock: false,
  enableNotifications: true,
  spellcheckEnabled: true,
  keyboardShortcuts: defaultKeyboardShortcuts,
  externalGoosed: {
    enabled: false,
    url: '',
    secret: '',
  },

  // UI preferences
  theme: 'light',
  useSystemTheme: true,
  language: 'system',
  responseStyle: 'concise',
  showPricing: true,
  seenAnnouncementIds: [],
  recentModels: [],
  useLegacyAgentLoop: false,
};

export function getKeyboardShortcuts(settings: Settings): KeyboardShortcuts {
  if (!settings.keyboardShortcuts && settings.globalShortcut !== undefined) {
    const focusShortcut = settings.globalShortcut;
    let launcherShortcut: string | null = null;

    if (focusShortcut) {
      if (focusShortcut.includes('Shift')) {
        launcherShortcut = focusShortcut;
      } else {
        launcherShortcut = focusShortcut.replace(/\+([Gg])$/, '+Shift+$1');
      }
    }

    return {
      ...defaultKeyboardShortcuts,
      focusWindow: focusShortcut,
      quickLauncher: launcherShortcut,
    };
  }
  return { ...defaultKeyboardShortcuts, ...settings.keyboardShortcuts };
}
