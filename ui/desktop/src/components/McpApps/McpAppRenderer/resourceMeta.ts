import type { McpUiResourceCsp } from '@modelcontextprotocol/ext-apps/app-bridge';
import type { SandboxPermissions } from '../types';

export interface ResourceMeta {
  csp: McpUiResourceCsp | null;
  permissions: SandboxPermissions | null;
  prefersBorder: boolean;
}
