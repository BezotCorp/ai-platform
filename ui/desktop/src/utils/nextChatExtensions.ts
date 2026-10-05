import type { NextChatExtensionDraft } from './nextChatExtensionDraft';
export type { NextChatExtensionDraft } from './nextChatExtensionDraft';

import type { ExtensionConfig } from '../types/extensionConfig';
import { configuredExtensionEntryToConfig } from './configuredExtensionEntryToConfig';
import type { ConfiguredExtensionEntry } from '../types/configuredExtensionEntry';



export function createNextChatExtensionDraft(
  allExtensions: ConfiguredExtensionEntry[] = []
): NextChatExtensionDraft {
  return {
    selectedNames: new Set(
      allExtensions.filter((extension) => extension.enabled).map((extension) => extension.name)
    ),
  };
}

export function selectNextChatExtensions(
  allExtensions: ConfiguredExtensionEntry[],
  draft: NextChatExtensionDraft
): ExtensionConfig[] {
  return allExtensions
    .filter((extension) => draft.selectedNames.has(extension.name))
    .map(configuredExtensionEntryToConfig);
}

export function isNextChatExtensionSelected(
  extension: ConfiguredExtensionEntry,
  draft: NextChatExtensionDraft
): boolean {
  return draft.selectedNames.has(extension.name);
}

export function toggleNextChatExtension(
  draft: NextChatExtensionDraft,
  extension: ConfiguredExtensionEntry
): NextChatExtensionDraft {
  const selectedNames = new Set(draft.selectedNames);
  if (selectedNames.has(extension.name)) {
    selectedNames.delete(extension.name);
  } else {
    selectedNames.add(extension.name);
  }
  return { selectedNames };
}
