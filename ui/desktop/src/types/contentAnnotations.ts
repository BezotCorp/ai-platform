import type { JsonObject, Role } from '.';

export type ContentAnnotations =
  | {
      audience?: Role[];
      lastModified?: string;
      priority?: number;
      _meta?: JsonObject;
    }
  | JsonObject;
