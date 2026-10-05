import React from 'react';
import type { View } from '../../utils/navigationUtils';
import { ChatState } from '../../types/chatState';
import { DroppedFile } from '../../hooks/useFileDrop';
import { Recipe } from '../../recipe';
import type { Message } from '../../types/message';
import type { UserInput } from '../../types/userInput';
import type { NextChatExtensionDraft } from '../../utils/nextChatExtensions';
import type { ChatInputLiveVoice } from '../ChatInput/chatInputLiveVoice';

export interface ChatInputProps {
  sessionId: string | null;
  handleSubmit: (input: UserInput) => void;
  chatState: ChatState;
  hasActiveRun: boolean;
  onStop?: () => void;
  onSteerQueuedMessage?: (input: UserInput) => Promise<boolean>;
  pauseQueueOnStop?: boolean;
  queueProcessingBlocked?: boolean;
  commandHistory?: string[];
  initialValue?: string;
  /**
   * Unsent input, held above the route outlet so it outlives the unmount.
   * Only New Chat passes it: every other chat stays mounted in
   * `ChatSessionsContainer` and keeps its text in local state.
   */
  draftRef?: React.RefObject<string>;
  droppedFiles?: DroppedFile[];
  onFilesProcessed?: () => void;
  setView: (view: View) => void;
  totalTokens?: number;
  contextLimit?: number;
  accumulatedInputTokens?: number;
  accumulatedOutputTokens?: number;
  accumulatedCost?: number | null;
  messages?: Message[];
  disableAnimation?: boolean;
  recipe?: Recipe | null;
  recipeId?: string | null;
  initialPrompt?: string;
  append?: (message: Message) => void;
  onWorkingDirChange?: (newDir: string) => Promise<void> | void;
  inputRef?: React.RefObject<HTMLTextAreaElement | null>;
  sessionModel?: string | null;
  sessionProvider?: string | null;
  sessionLoaded?: boolean;
  workingDir?: string | null;
  latestInference?: Message['metadata']['inference'] | null;
  nextChatExtensionDraft?: NextChatExtensionDraft;
  onNextChatExtensionDraftChange?: (draft: NextChatExtensionDraft) => void;
  liveVoice?: ChatInputLiveVoice;
  appendQuote?: string | null;
  onAppendQuoteConsumed?: () => void;
}
