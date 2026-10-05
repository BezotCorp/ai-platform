import React from 'react';
import { ChatType } from '../types/chatType';
import type { UserInput } from '../types/userInput';
import type { LiveVoiceController } from '../liveVoice/useLiveVoice';

export interface BaseChatProps {
  setChat: (chat: ChatType) => void;
  onMessageSubmit?: (message: string) => void;
  renderHeader?: () => React.ReactNode;
  customChatInputProps?: Record<string, unknown>;
  customMainLayoutProps?: Record<string, unknown>;
  contentClassName?: string;
  disableSearch?: boolean;
  suppressEmptyState: boolean;
  sessionId: string;
  isActiveSession: boolean;
  initialMessage?: UserInput;
  noAutoSubmit?: boolean;
  liveVoice: LiveVoiceController;
}
