import type { ReactNode } from 'react';
import type { ChatType } from '../../types/chatType';

export interface ChatProviderProps {
  children: ReactNode;
  chat: ChatType;
  setChat: (chat: ChatType) => void;
  contextKey?: string; // Optional context key, defaults to 'hub'
}
