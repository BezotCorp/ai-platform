import type { JsonObject } from './jsonObject';
import type { Role } from './role';

export type ContentAnnotations =
  | {
      audience?: Role[];
      lastModified?: string;
      priority?: number;
      _meta?: JsonObject;
    }
  | JsonObject;
