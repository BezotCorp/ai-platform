import type { ThemeTokens } from './themeTokens';
import type { ThemeVariant } from './themeVariant';

export interface ThemeDefinition {
  variant: ThemeVariant;
  tokens: ThemeTokens;
}
