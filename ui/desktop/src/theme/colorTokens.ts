import type { ThemeTokens } from './themeTokens';
import type { ColorTokenKey } from './colorTokenKey';

export type ColorTokens = Pick<ThemeTokens, ColorTokenKey>;
