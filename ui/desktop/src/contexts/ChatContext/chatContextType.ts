import type { ChatType } from '../../types/chatType';
import type { Recipe } from '../../recipe';

export interface ChatContextType {
  chat: ChatType;
  setChat: (chat: ChatType) => void;
  resetChat: () => void;
  hasActiveSession: boolean;
  setRecipe: (recipe: Recipe | null) => void;
  clearRecipe: () => void;
  // Context identification
  contextKey: string; // 'hub' or 'pair-{sessionId}'
}
