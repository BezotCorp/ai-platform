import type { CspMetadata } from './cspMetadata';
import type { PermissionsMetadata } from './permissionsMetadata';

export type UiMetadata = {
  csp?: CspMetadata | null;
  domain?: string | null;
  permissions?: PermissionsMetadata;
  prefersBorder?: boolean | null;
};
