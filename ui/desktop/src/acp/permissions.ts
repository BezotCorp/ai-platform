import type {
  ToolListItem,
  ToolPermissionEntry,
  ToolPermissionLevelKey,
} from '@aaif/goose-acp-client';
import { getAcpClient } from './acpConnection';

export type { ToolListItem, ToolPermissionEntry, ToolPermissionLevelKey };

export async function listTools(
  sessionId: string,
  extensionName?: string
): Promise<ToolListItem[]> {
  const client = await getAcpClient();
  const response = await client.goose.toolsListUnstable({
    sessionId,
    extensionName: extensionName ?? null,
  });
  return response.tools ?? [];
}

export async function setToolPermissions(toolPermissions: ToolPermissionEntry[]): Promise<void> {
  const client = await getAcpClient();
  await client.goose.toolsPermissionsSetUnstable({ toolPermissions });
}
