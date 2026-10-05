import type { ThemeId } from '../../theme/theme-tokens';
import type { McpUiHostStyles } from '@modelcontextprotocol/ext-apps/app-bridge';
import type { ThemePreference } from './themePreference';
import type { ResolvedTheme } from './resolvedTheme';

export interface ThemeContextValue {
  userThemePreference: ThemePreference;
  setUserThemePreference: (pref: ThemePreference) => void;
  resolvedThemeId: ThemeId;
  resolvedTheme: ResolvedTheme;
  mcpHostStyles: McpUiHostStyles;
}
