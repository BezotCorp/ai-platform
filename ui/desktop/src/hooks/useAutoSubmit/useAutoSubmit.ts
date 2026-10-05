import { AppEvents } from '../../constants/appEvents';
import {
  useCallback,
  useEffect,
  useRef,
} from 'react';
import { useSearchParams } from 'react-router';
import { ChatState } from '../../types/chatState';


import type { Session } from '../../types/session';


import type { UseAutoSubmitProps } from '../useAutoSubmit/useAutoSubmitProps';
import type { UseAutoSubmitReturn } from '../useAutoSubmit/useAutoSubmitReturn';
export function useAutoSubmit({
  sessionId,
  session,
  messages,
  chatState,
  initialMessage,
  canAutoSubmit = true,
  handleSubmit,
}: UseAutoSubmitProps): UseAutoSubmitReturn {
  const [searchParams] = useSearchParams();
  const hasAutoSubmittedRef = useRef(false);

  // Reset auto-submit flag when session changes
  useEffect(() => {
    hasAutoSubmittedRef.current = false;
  }, [sessionId]);

  const clearInitialMessage = useCallback(() => {
    window.dispatchEvent(
      new CustomEvent(AppEvents.CLEAR_INITIAL_MESSAGE, {
        detail: { sessionId },
      })
    );
  }, [sessionId]);

  const hasUnfilledParameters = useCallback((session: Session) => {
    if (session.session_type === 'scheduled') {
      return false;
    }

    const recipe = session.recipe;
    return recipe?.parameters && recipe.parameters.length > 0 && !session.user_recipe_values;
  }, []);

  // Auto-submit logic
  useEffect(() => {
    const currentSessionId = searchParams.get('resumeSessionId');
    const isCurrentSession = currentSessionId === sessionId;
    const shouldStartAgent = isCurrentSession && searchParams.get('shouldStartAgent') === 'true';

    if (!session || hasAutoSubmittedRef.current) {
      return;
    }

    if (!canAutoSubmit) {
      return;
    }

    if (chatState !== ChatState.Idle) {
      return;
    }

    // Scenario 1: New session with initial message from Hub
    // Hub always creates new sessions, so message_count will be 0
    if (initialMessage && session.message_count === 0 && messages.length === 0) {
      if (!hasUnfilledParameters(session)) {
        hasAutoSubmittedRef.current = true;
        handleSubmit(initialMessage);
        clearInitialMessage();
      }
      return;
    }

    // Scenario 2: Forked session with edited message
    if (shouldStartAgent && initialMessage) {
      if (messages.length > 0) {
        hasAutoSubmittedRef.current = true;
        handleSubmit(initialMessage);
        clearInitialMessage();
        return;
      }
      return;
    }

    // Scenario 3: Resume with shouldStartAgent (continue existing conversation)
    if (shouldStartAgent) {
      if (!hasUnfilledParameters(session)) {
        hasAutoSubmittedRef.current = true;
        handleSubmit({ msg: '', images: [] });
      }
      return;
    }
  }, [
    session,
    initialMessage,
    searchParams,
    handleSubmit,
    sessionId,
    messages.length,
    chatState,
    canAutoSubmit,
    clearInitialMessage,
    hasUnfilledParameters,
  ]);

  return {
    hasAutoSubmitted: hasAutoSubmittedRef.current,
  };
}
