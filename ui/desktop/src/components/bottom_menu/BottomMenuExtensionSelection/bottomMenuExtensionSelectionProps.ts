import { type NextChatExtensionDraft } from '../../../utils/nextChatExtensions';

export interface BottomMenuExtensionSelectionProps {
  sessionId: string | null;
  nextChatExtensionDraft?: NextChatExtensionDraft;
  onNextChatExtensionDraftChange?: (draft: NextChatExtensionDraft) => void;
}
