import type { ResourceMetadata } from '.';

export type McpAppResource = {
  _meta?: ResourceMetadata | null;
  blob?: string | null;
  description?: string | null;
  mimeType: string;
  name: string;
  text?: string | null;
  uri: string;
};
