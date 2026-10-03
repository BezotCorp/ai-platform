import { contextBridge, ipcRenderer, webUtils, type IpcRendererEvent } from 'electron';
import type { Recipe } from './recipe';
import type { GooseApp } from './types/mcpAppResource';
import { defaultSettings, type Settings } from './utils/settings';
import type { OpenExternalUrlResult } from './utils/openExternalUrlResult';
import type { AppConfigAPI } from './appConfigApi';
import type { ElectronAPI } from './electronApi';
import type { CreateChatWindowOptions } from './createChatWindowsOptions';
import type { NotificationData } from './notificationData';
import type { MessageBoxOptions } from './messageBoxOptions';
import type { SaveDialogOptions } from './saveDialogOptions';
import type { UpdaterEvent } from './updaterEvent';
import type { SettingKey } from './utils/settingKey';

// Mapping from settings keys to their old localStorage keys for lazy migration
const localStorageKeyMap: Partial<Record<SettingKey, string>> = {
  theme: 'theme',
  useSystemTheme: 'use_system_theme',
  responseStyle: 'response_style',
  showPricing: 'show_pricing',
  seenAnnouncementIds: 'seenAnnouncementIds',
};

type LocalStorageParserMap = {
  [K in SettingKey]?: (rawValue: string) => Settings[K] | null;
};

const localStorageParsers: LocalStorageParserMap = {
  theme: (rawValue) => (rawValue === 'dark' || rawValue === 'light' ? rawValue : null),
  useSystemTheme: (rawValue) => rawValue === 'true',
  responseStyle: (rawValue) => rawValue,
  showPricing: (rawValue) => rawValue === 'true',
  seenAnnouncementIds: (rawValue) => {
    const parsed: unknown = JSON.parse(rawValue);
    if (
      !Array.isArray(parsed) ||
      !parsed.every((value): value is string => typeof value === 'string')
    ) {
      return null;
    }
    return parsed;
  },
};

function parseLocalStorageValue<K extends SettingKey>(
  key: K,
  rawValue: string
): Settings[K] | null {
  const parser = localStorageParsers[key];
  if (!parser) {
    return null;
  }
  try {
    return parser(rawValue);
  } catch {
    return null;
  }
}

const config = JSON.parse(process.argv.find((arg) => arg.startsWith('{')) || '{}');

const electronAPI: ElectronAPI = {
  platform: process.platform,
  arch: process.arch,
  reactReady: () => ipcRenderer.send('react-ready'),
  getConfig: () => {
    if (!config || Object.keys(config).length === 0) {
      console.warn(
        'No config provided by main process. This may indicate an initialization issue.'
      );
    }
    return config;
  },
  hideWindow: () => ipcRenderer.send('hide-window'),
  directoryChooser: () => ipcRenderer.invoke('directory-chooser'),
  createChatWindow: (options?: CreateChatWindowOptions) =>
    ipcRenderer.send('create-chat-window', options || {}),
  logInfo: (txt: string) => ipcRenderer.send('logInfo', txt),
  showNotification: (data: NotificationData) => ipcRenderer.send('notify', data),
  showMessageBox: (options: MessageBoxOptions) => ipcRenderer.invoke('show-message-box', options),
  showSaveDialog: (options: SaveDialogOptions) => ipcRenderer.invoke('show-save-dialog', options),
  openInChrome: (url: string) => ipcRenderer.send('open-in-chrome', url),
  reloadApp: () => ipcRenderer.send('reload-app'),
  checkForOllama: () => ipcRenderer.invoke('check-ollama'),
  selectFileOrDirectory: (defaultPath?: string) =>
    ipcRenderer.invoke('select-file-or-directory', defaultPath),
  selectImportSessionFile: () => ipcRenderer.invoke('select-import-session-file'),
  getBinaryPath: (binaryName: string) => ipcRenderer.invoke('get-binary-path', binaryName),
  selectRecipeFile: () => ipcRenderer.invoke('select-recipe-file'),
  readGoosehints: () => ipcRenderer.invoke('read-goosehints'),
  writeGoosehints: (content: string) => ipcRenderer.invoke('write-goosehints', content),
  writeFile: (filePath: string, content: string) =>
    ipcRenderer.invoke('write-file', filePath, content),
  ensureDirectory: (dirPath: string) => ipcRenderer.invoke('ensure-directory', dirPath),
  listFiles: (dirPath: string, extension?: string) =>
    ipcRenderer.invoke('list-files', dirPath, extension),
  getPathForFile: (file: File) => webUtils.getPathForFile(file),
  getAllowedExtensions: () => ipcRenderer.invoke('get-allowed-extensions'),
  setMenuBarIcon: (show: boolean) => ipcRenderer.invoke('set-menu-bar-icon', show),
  getMenuBarIconState: () => ipcRenderer.invoke('get-menu-bar-icon-state'),
  setDockIcon: (show: boolean) => ipcRenderer.invoke('set-dock-icon', show),
  getDockIconState: () => ipcRenderer.invoke('get-dock-icon-state'),
  getSetting: async <K extends SettingKey>(key: K): Promise<Settings[K]> => {
    try {
      // Check for localStorage value first (lazy migration)
      const localStorageKey = localStorageKeyMap[key];
      if (localStorageKey) {
        const rawValue = localStorage.getItem(localStorageKey);
        if (rawValue !== null) {
          const parsed = parseLocalStorageValue(key, rawValue);
          if (parsed !== null) {
            return parsed;
          }
        }
      }
      return await ipcRenderer.invoke('get-setting', key);
    } catch (error) {
      console.error(`Failed to get setting '${key}', using default`, error);
      return defaultSettings[key];
    }
  },
  setSetting: async <K extends SettingKey>(key: K, value: Settings[K]): Promise<void> => {
    // Clear any localStorage version when writing
    const localStorageKey = localStorageKeyMap[key];
    if (localStorageKey) {
      localStorage.removeItem(localStorageKey);
    }
    return ipcRenderer.invoke('set-setting', key, value);
  },
  getSecretKey: () => ipcRenderer.invoke('get-secret-key'),
  getAcpUrl: () => ipcRenderer.invoke('get-acp-url'),
  setWakelock: (enable: boolean) => ipcRenderer.invoke('set-wakelock', enable),
  getWakelockState: () => ipcRenderer.invoke('get-wakelock-state'),
  setSpellcheck: (enable: boolean) => ipcRenderer.invoke('set-spellcheck', enable),
  getSpellcheckState: () => ipcRenderer.invoke('get-spellcheck-state'),
  openNotificationsSettings: () => ipcRenderer.invoke('open-notifications-settings'),
  isAnyWindowFocused: () => ipcRenderer.invoke('is-any-window-focused'),
  getIsFullScreen: () => ipcRenderer.invoke('get-is-fullscreen'),
  onMouseBackButtonClicked: (callback: () => void) => {
    // Wrapper that ignores the event parameter.
    const wrappedCallback = (_event: IpcRendererEvent) => callback();
    ipcRenderer.on('mouse-back-button-clicked', wrappedCallback);
    return wrappedCallback;
  },
  offMouseBackButtonClicked: (callback: () => void) => {
    ipcRenderer.removeListener('mouse-back-button-clicked', callback);
  },
  on: (channel: string, callback: (event: IpcRendererEvent, ...args: unknown[]) => void) => {
    ipcRenderer.on(channel, callback);
  },
  off: (channel: string, callback: (event: IpcRendererEvent, ...args: unknown[]) => void) => {
    ipcRenderer.off(channel, callback);
  },
  emit: (channel: string, ...args: unknown[]) => {
    ipcRenderer.emit(channel, ...args);
  },
  broadcastThemeChange: (themeData: {
    mode: string;
    useSystemTheme: boolean;
    theme: string;
    tokensUpdated?: boolean;
  }) => {
    ipcRenderer.send('broadcast-theme-change', themeData);
  },
  openExternal: (url: string): Promise<OpenExternalUrlResult> => {
    return ipcRenderer.invoke('open-external', url);
  },
  getVersion: (): string => {
    return config.GOOSE_VERSION || ipcRenderer.sendSync('get-app-version') || '';
  },
  checkForUpdates: (): Promise<{ updateInfo: unknown; error: string | null }> => {
    return ipcRenderer.invoke('check-for-updates');
  },
  downloadUpdate: (): Promise<{ success: boolean; error: string | null }> => {
    return ipcRenderer.invoke('download-update');
  },
  installUpdate: (): void => {
    void ipcRenderer.invoke('install-update');
  },
  restartApp: (): void => {
    ipcRenderer.send('restart-app');
  },
  onUpdaterEvent: (callback: (event: UpdaterEvent) => void): void => {
    ipcRenderer.on('updater-event', (_event, data) => callback(data));
  },
  getUpdateState: (): Promise<{ updateAvailable: boolean; latestVersion?: string } | null> => {
    return ipcRenderer.invoke('get-update-state');
  },
  isUsingGitHubFallback: (): Promise<boolean> => {
    return ipcRenderer.invoke('is-using-github-fallback');
  },
  getAutoDownloadDisabled: (): Promise<boolean> => {
    return ipcRenderer.invoke('get-auto-download-disabled');
  },
  closeWindow: () => ipcRenderer.send('close-window'),
  hasAcceptedRecipeBefore: (recipe: Recipe) =>
    ipcRenderer.invoke('has-accepted-recipe-before', recipe),
  recordRecipeHash: (recipe: Recipe) => ipcRenderer.invoke('record-recipe-hash', recipe),
  openDirectoryInExplorer: (directoryPath: string) =>
    ipcRenderer.invoke('open-directory-in-explorer', directoryPath),
  launchApp: (app: GooseApp) => ipcRenderer.invoke('launch-app', app),
  refreshApp: (app: GooseApp) => ipcRenderer.invoke('refresh-app', app),
  closeApp: (appName: string) => ipcRenderer.invoke('close-app', appName),
  addRecentDir: (dir: string) => ipcRenderer.invoke('add-recent-dir', dir),
  listRecentDirs: () => ipcRenderer.invoke('list-recent-dirs'),
  listGitWorktreeDirs: (dir: string) => ipcRenderer.invoke('list-git-worktree-dirs', dir),
  getGitBranchInfo: (dir: string) => ipcRenderer.invoke('get-git-branch-info', dir),
  listGitBranches: (dir: string) => ipcRenderer.invoke('list-git-branches', dir),
  switchGitBranch: (dir: string, branch: string) =>
    ipcRenderer.invoke('switch-git-branch', dir, branch),
};

function getAppLocale(): unknown {
  try {
    return ipcRenderer.sendSync('get-app-locale') ?? config.GOOSE_LOCALE;
  } catch {
    return config.GOOSE_LOCALE;
  }
}

const appConfigAPI: AppConfigAPI = {
  get: (key: string) => (key === 'GOOSE_LOCALE' ? getAppLocale() : config[key]),
  getAll: () => ({ ...config, GOOSE_LOCALE: getAppLocale() }),
};
// Expose the APIs
contextBridge.exposeInMainWorld('electron', electronAPI);
contextBridge.exposeInMainWorld('appConfig', appConfigAPI);
// Type declaration for TypeScript
declare global {
  interface Window {
    electron: ElectronAPI;
    appConfig: AppConfigAPI;
  }
}
