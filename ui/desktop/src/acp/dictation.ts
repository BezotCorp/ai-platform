import type { DictationProviderStatusEntry } from '@bezotcorp/bcaip-acp-client';
import { getAcpClient } from './acpConnection';

import type { DictationProviders } from './dictationProviders';
import type { LocalDictationModel } from './localDictationModel';
import type { LocalDictationDownloadProgress } from './localDictationDownloadProgress';
export type { DictationProviderStatusEntry };

export async function getDictationConfig(): Promise<DictationProviders> {
  const client = await getAcpClient();
  const response = await client.bcaip.dictationConfigUnstable({});
  return response.providers ?? {};
}

export async function transcribeDictation(
  audio: string,
  mimeType: string,
  provider: string
): Promise<string> {
  const client = await getAcpClient();
  const response = await client.bcaip.dictationTranscribeUnstable({ audio, mimeType, provider });
  return response.text;
}

export async function listLocalDictationModels(): Promise<LocalDictationModel[]> {
  const client = await getAcpClient();
  const response = await client.bcaip.dictationModelsListUnstable({});
  return response.models;
}

export async function downloadLocalDictationModel(modelId: string): Promise<void> {
  const client = await getAcpClient();
  await client.bcaip.dictationModelsDownloadUnstable({ modelId });
}

export async function getLocalDictationModelDownloadProgress(
  modelId: string
): Promise<LocalDictationDownloadProgress | null> {
  const client = await getAcpClient();
  const response = await client.bcaip.dictationModelsDownloadProgressUnstable({ modelId });
  return response.progress ?? null;
}

export async function cancelLocalDictationModelDownload(modelId: string): Promise<void> {
  const client = await getAcpClient();
  await client.bcaip.dictationModelsCancelUnstable({ modelId });
}

export async function deleteLocalDictationModel(modelId: string): Promise<void> {
  const client = await getAcpClient();
  await client.bcaip.dictationModelsDeleteUnstable({ modelId });
}
