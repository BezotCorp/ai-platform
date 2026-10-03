import type { ActionRequired, Message, ToolConfirmationRequestContent } from '.';
import { getToolResponses } from '.';

export interface ToolConfirmationData {
  generation?: string;
  id: string;
  toolName: string;
  arguments: Record<string, unknown>;
  prompt?: string | null;
}

export function getToolConfirmationContent(
  message: Message
): (ActionRequired & { type: 'actionRequired' }) | undefined {
  return message.content.find(
    (content): content is ActionRequired & { type: 'actionRequired' } =>
      content.type === 'actionRequired' && content.data.actionType === 'toolConfirmation'
  );
}

export function getToolConfirmationRequestContent(
  message: Message
): ToolConfirmationRequestContent | undefined {
  return message.content.find(
    (content): content is ToolConfirmationRequestContent =>
      content.type === 'toolConfirmationRequest'
  );
}

export function getAnyToolConfirmationData(message: Message): ToolConfirmationData | undefined {
  const confirmationRequest = getToolConfirmationRequestContent(message);
  if (confirmationRequest) {
    return {
      id: confirmationRequest.id,
      toolName: confirmationRequest.toolName,
      arguments: confirmationRequest.arguments,
      prompt: confirmationRequest.prompt,
    };
  }

  const actionRequired = getToolConfirmationContent(message);
  if (actionRequired && actionRequired.data.actionType === 'toolConfirmation') {
    return {
      generation: actionRequired.data.generation,
      id: actionRequired.data.id,
      toolName: actionRequired.data.toolName,
      arguments: actionRequired.data.arguments,
      prompt: actionRequired.data.prompt,
    };
  }

  return undefined;
}

export function getPendingToolConfirmationIds(messages: Message[]): Set<string> {
  const pendingIds = new Set<string>();
  const respondedIds = new Set<string>();

  for (const message of messages) {
    const responses = getToolResponses(message);
    for (const response of responses) {
      respondedIds.add(response.id);
    }
  }

  for (const message of messages) {
    const confirmationData = getAnyToolConfirmationData(message);
    if (confirmationData && !respondedIds.has(confirmationData.id)) {
      pendingIds.add(confirmationData.id);
    }
  }

  return pendingIds;
}
