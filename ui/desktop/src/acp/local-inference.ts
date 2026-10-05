import { getAcpClient } from './acpConnection';

import type { LocalModelResponse } from './localModelResponse';
import type { DownloadProgress } from './downloadProgress';
import type { DownloadModelRequest } from './downloadModelRequest';
import type { HfModelInfo } from './hfModelInfo';
import type { ModelSettings } from './modelSettings';
import type { RepoVariantsResponse } from './repoVariantsResponse';
export async function listLocalModels(): Promise<LocalModelResponse[]> {
  const client = await getAcpClient();
  const response = await client.goose.localInferenceModelsListUnstable({});
  return response.models;
}

export async function downloadHfModel(request: DownloadModelRequest): Promise<string> {
  const client = await getAcpClient();
  const response = await client.goose.localInferenceModelsDownloadUnstable(request);
  return response.modelId;
}

export async function getLocalModelDownloadProgress(
  modelId: string
): Promise<DownloadProgress | null> {
  const client = await getAcpClient();
  const response = await client.goose.localInferenceModelsDownloadProgressUnstable({ modelId });
  return response.progress ?? null;
}

export async function cancelLocalModelDownload(modelId: string): Promise<void> {
  const client = await getAcpClient();
  await client.goose.localInferenceModelsDownloadCancelUnstable({ modelId });
}

export async function deleteLocalModel(modelId: string): Promise<void> {
  const client = await getAcpClient();
  await client.goose.localInferenceModelsDeleteUnstable({ modelId });
}

export async function evictLocalModel(modelId: string): Promise<void> {
  const client = await getAcpClient();
  await client.goose.localInferenceModelsEvictUnstable({ modelId });
}

export async function getModelSettings(modelId: string): Promise<ModelSettings> {
  const client = await getAcpClient();
  const response = await client.goose.localInferenceModelsSettingsReadUnstable({ modelId });
  return response.settings;
}

export async function updateModelSettings(
  modelId: string,
  settings: ModelSettings
): Promise<ModelSettings> {
  const client = await getAcpClient();
  const response = await client.goose.localInferenceModelsSettingsUpdateUnstable({
    modelId,
    settings,
  });
  return response.settings;
}

export async function searchHfModels(query: string, limit?: number): Promise<HfModelInfo[]> {
  const client = await getAcpClient();
  const response = await client.goose.localInferenceHuggingfaceSearchUnstable({ query, limit });
  return response.models;
}

export async function getRepoFiles(repoId: string): Promise<RepoVariantsResponse> {
  const client = await getAcpClient();
  const response = await client.goose.localInferenceHuggingfaceRepoVariantsUnstable({ repoId });
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
  const response = await client.goose.localInferenceChatTemplatesBuiltinListUnstable({});
  return response.templates;
}
