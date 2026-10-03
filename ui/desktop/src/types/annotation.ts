import type { Role } from './role';

export type Annotations = {
  audience?: Role[];
  lastModified?: string;
  priority?: number;
};
