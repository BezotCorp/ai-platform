import React, { createContext, useContext } from 'react';

import { Recipe } from '../../recipe';

import type { ChatContextType } from '../ChatContext/chatContextType';
import type { ChatProviderProps } from '../ChatContext/chatProviderProps';
// TODO(Douwe): We should not need this anymore
export const DEFAULT_CHAT_TITLE = 'New Chat';


const ChatContext = createContext<ChatContextType | undefined>(undefined);


export const ChatProvider: React.FC<ChatProviderProps> = ({
  children,
  chat,
  setChat,
  contextKey = 'hub',
}) => {
  const resetChat = () => {
    setChat({
      sessionId: '',
      name: DEFAULT_CHAT_TITLE,
      messages: [],
      recipe: null,
      recipeParameterValues: null,
    });
  };

  const setRecipe = (recipe: Recipe | null) => {
    setChat({
      ...chat,
      recipe: recipe,
      recipeParameterValues: null,
    });
  };

  const clearRecipe = () => {
    setChat({
      ...chat,
      recipe: null,
    });
  };

  const hasActiveSession = chat.messages.length > 0;

  const value: ChatContextType = {
    chat,
    setChat,
    resetChat,
    hasActiveSession,
    setRecipe,
    clearRecipe,
    contextKey,
  };

  return <ChatContext.Provider value={value}>{children}</ChatContext.Provider>;
};

export const useChatContext = (): ChatContextType | null => {
  const context = useContext(ChatContext);
  return context || null;
};
