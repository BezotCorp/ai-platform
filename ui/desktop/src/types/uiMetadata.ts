import type { CspMetadata, PermissionsMetadata } from '.';

export type UiMetadata = {
  csp?: CspMetadata | null;
  domain?: string | null;
  permissions?: PermissionsMetadata;
  prefersBorder?: boolean | null;
};
