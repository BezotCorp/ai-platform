import { getAcpClient } from './acpConnection';

import type { LocalModelResponse } from './localModelResponse';
import type { DownloadProgress } from './downloadProgress';
import type { DownloadModelRequest } from './downloadModelRequest';
import type { HfModelInfo } from './hfModelInfo';
import type { ModelSettings } from './modelSettings';
import type { RepoVariantsResponse } from './repoVariantsResponse';
export async function listLocalModels(): Promise<LocalModelResponse[]> {
  const client = await getAcpClient();
  const response = await client.bcaip.localInferenceModelsListUnstable({});
  return response.models;
}

export async function downloadHfModel(request: DownloadModelRequest): Promise<string> {
  const client = await getAcpClient();
  const response = await client.bcaip.localInferenceModelsDownloadUnstable(request);
  return response.modelId;
}

export async function getLocalModelDownloadProgress(
  modelId: string
): Promise<DownloadProgress | null> {
  const client = await getAcpClient();
  const response = await client.bcaip.localInferenceModelsDownloadProgressUnstable({ modelId });
  return response.progress ?? null;
}

export async function cancelLocalModelDownload(modelId: string): Promise<void> {
  const client = await getAcpClient();
  await client.bcaip.localInferenceModelsDownloadCancelUnstable({ modelId });
}

export async function deleteLocalModel(modelId: string): Promise<void> {
  const client = await getAcpClient();
  await client.bcaip.localInferenceModelsDeleteUnstable({ modelId });
}

export async function evictLocalModel(modelId: string): Promise<void> {
  const client = await getAcpClient();
  await client.bcaip.localInferenceModelsEvictUnstable({ modelId });
}

export async function getModelSettings(modelId: string): Promise<ModelSettings> {
  const client = await getAcpClient();
  const response = await client.bcaip.localInferenceModelsSettingsReadUnstable({ modelId });
  return response.settings;
}

export async function updateModelSettings(
  modelId: string,
  settings: ModelSettings
): Promise<ModelSettings> {
  const client = await getAcpClient();
  const response = await client.bcaip.localInferenceModelsSettingsUpdateUnstable({
    modelId,
    settings,
  });
  return response.settings;
}

export async function searchHfModels(query: string, limit?: number): Promise<HfModelInfo[]> {
  const client = await getAcpClient();
  const response = await client.bcaip.localInferenceHuggingfaceSearchUnstable({ query, limit });
  return response.models;
}

export async function getRepoFiles(repoId: string): Promise<RepoVariantsResponse> {
  const client = await getAcpClient();
  const response = await client.bcaip.localInferenceHuggingfaceRepoVariantsUnstable({ repoId });
  return {
    variants: response.variants,
    recommendedIndex: response.recommendedIndex ?? null,
    availableMemoryBytes: response.availableMemoryBytes,
    downloadedQuants: response.downloadedQuants,
    downloadedVariants: response.downloadedVariants,
  };
}

export async function listBuiltinChatTemplates(): Promise<string[]> {
  const client = await getAcpClient();
  const response = await client.bcaip.localInferenceChatTemplatesBuiltinListUnstable({});
  return response.templates;
}
