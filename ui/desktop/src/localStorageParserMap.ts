import { type Settings } from './utils/settings';
import type { SettingKey } from './utils/settingKey';

export type LocalStorageParserMap = {
  [K in SettingKey]?: (rawValue: string) => Settings[K] | null;
};
