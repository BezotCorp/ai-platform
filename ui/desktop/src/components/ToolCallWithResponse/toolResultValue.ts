import type { ContentBlock } from '../../types/contentBlock';
import type { UiMeta } from './uiMeta';

export type ToolResultValue = {
  content: ContentBlock[];
  structuredContent?: unknown;
  isError: boolean;
  _meta?: UiMeta;
};
