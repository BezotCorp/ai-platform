import type { JsonObject } from '.';

export type ResourceContents =
  | {
      _meta?: JsonObject;
      mimeType?: string;
      text: string;
      uri: string;
    }
  | {
      _meta?: JsonObject;
      blob: string;
      mimeType?: string;
      uri: string;
    };
