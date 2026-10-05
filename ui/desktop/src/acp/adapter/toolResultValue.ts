import type { ContentBlock as GooseContentBlock } from '../../types/contentBlock';
import type { DesktopMcpAppMeta } from './desktopMcpAppMeta';

export type ToolResultValue = {
  content: GooseContentBlock[];
  structuredContent?: unknown;
  isError: boolean;
  _meta?: DesktopMcpAppMeta;
};
