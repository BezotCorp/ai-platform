import type { CspMetadata } from './cspMetadata';
import type { PermissionsMetadata } from './permissionMetadata';

export type UiMetadata = {
  csp?: CspMetadata | null;
  domain?: string | null;
  permissions?: PermissionsMetadata;
  prefersBorder?: boolean | null;
};
