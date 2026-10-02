import type { GetPromptResponseUnstable, PromptTemplateEntry } from '@aaif/goose-acp-client';
import { getAcpClient } from './acpConnection';

export type PromptTemplate = PromptTemplateEntry;
export type PromptContent = GetPromptResponseUnstable;

export async function acpListPrompts(): Promise<PromptTemplate[]> {
  const client = await getAcpClient();
  const response = await client.goose.configPromptsListUnstable({});
  return response.prompts;
}

export async function acpGetPrompt(name: string): Promise<PromptContent> {
  const client = await getAcpClient();
  return client.goose.configPromptsGetUnstable({ name });
}

export async function acpSavePrompt(name: string, content: string): Promise<void> {
  const client = await getAcpClient();
  await client.goose.configPromptsSaveUnstable({ name, content });
}

export async function acpResetPrompt(name: string): Promise<void> {
  const client = await getAcpClient();
  await client.goose.configPromptsResetUnstable({ name });
}
