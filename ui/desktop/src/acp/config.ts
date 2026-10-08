import type { ConfigReadValue } from './configReadValue';
export type { ConfigReadValue } from './configReadValue';

import { getAcpClient } from './acpConnection';


export async function acpReadConfig(
  key: string,
  isSecret: boolean = false
): Promise<ConfigReadValue> {
  const client = await getAcpClient();
  const { value } = await client.bcaip.configReadUnstable({ key, isSecret });
  if (value == null) {
    return null;
  }
  if (isSecret) {
    return { maskedValue: value as string };
  }
  return value;
}

export async function acpUpsertConfig(
  key: string,
  value: unknown,
  isSecret: boolean = false
): Promise<void> {
  const client = await getAcpClient();
  await client.bcaip.configUpsertUnstable({ key, value, isSecret });
}

export async function acpRemoveConfig(key: string, isSecret: boolean): Promise<void> {
  const client = await getAcpClient();
  await client.bcaip.configRemoveUnstable({ key, isSecret });
}

export async function acpReadAllConfig(): Promise<Record<string, unknown>> {
  const client = await getAcpClient();
  const { config } = await client.bcaip.configReadAllUnstable({});
  return config;
}
