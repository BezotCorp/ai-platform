import { getAcpClient } from './acpConnection';

export type ConfigReadValue = unknown;

export async function acpReadConfig(
  key: string,
  isSecret: boolean = false
): Promise<ConfigReadValue> {
  const client = await getAcpClient();
  const { value } = await client.goose.configReadUnstable({ key, isSecret });
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
  await client.goose.configUpsertUnstable({ key, value, isSecret });
}

export async function acpRemoveConfig(key: string, isSecret: boolean): Promise<void> {
  const client = await getAcpClient();
  await client.goose.configRemoveUnstable({ key, isSecret });
}

export async function acpReadAllConfig(): Promise<Record<string, unknown>> {
  const client = await getAcpClient();
  const { config } = await client.goose.configReadAllUnstable({});
  return config;
}
