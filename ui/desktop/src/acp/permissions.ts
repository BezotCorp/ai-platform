import type {
  ToolListItem,
  ToolPermissionEntry,
  ToolPermissionLevelKey,
} from '@bezotcorp/bcaip-acp-client';
import { getAcpClient } from './acpConnection';

export type { ToolListItem, ToolPermissionEntry, ToolPermissionLevelKey };

export async function listTools(
  sessionId: string,
  extensionName?: string
): Promise<ToolListItem[]> {
  const client = await getAcpClient();
  const response = await client.bcaip.toolsListUnstable({
    sessionId,
    extensionName: extensionName ?? null,
  });
  return response.tools ?? [];
}

export async function setToolPermissions(toolPermissions: ToolPermissionEntry[]): Promise<void> {
  const client = await getAcpClient();
  await client.bcaip.toolsPermissionsSetUnstable({ toolPermissions });
}
