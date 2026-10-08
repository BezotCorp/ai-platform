import type { ExtensionConfig } from '../types/extensionConfig';
import type { SessionExtension } from './sessionExtension';
export type { SessionExtension } from './sessionExtension';
import { getAcpClient } from './acpConnection';
import { extensionConfigToBcaipExtension, bcaipExtensionToExtensionConfig } from './extensions';

export async function getSessionExtensions(sessionId: string): Promise<SessionExtension[]> {
  const client = await getAcpClient();
  const response = await client.bcaip.sessionExtensionsListUnstable({ sessionId });
  const extensionKeys = new Set<string>();
  const extensions: SessionExtension[] = [];

  for (const entry of response.extensions) {
    if (extensionKeys.has(entry.extensionKey)) {
      throw new Error(`Duplicate session extension key '${entry.extensionKey}'`);
    }
    extensionKeys.add(entry.extensionKey);

    const config = bcaipExtensionToExtensionConfig(entry.extension);
    if (config) {
      extensions.push({ ...config, extensionKey: entry.extensionKey });
    }
  }

  return extensions;
}

export async function addSessionExtension(
  sessionId: string,
  config: ExtensionConfig
): Promise<void> {
  const extension = extensionConfigToBcaipExtension(config);
  if (!extension) {
    throw new Error(`Unsupported extension type for ACP: ${config.type}`);
  }
  const client = await getAcpClient();
  await client.bcaip.sessionExtensionsAddUnstable({ sessionId, extension });
}

export async function removeSessionExtension(
  sessionId: string,
  extensionKey: string
): Promise<void> {
  const client = await getAcpClient();
  await client.bcaip.sessionExtensionsRemoveUnstable({ sessionId, extensionKey });
}
