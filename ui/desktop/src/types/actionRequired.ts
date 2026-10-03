import type { ActionRequiredData } from './actionRequiredData';
import type { Message } from './message';

export type ActionRequired = {
  data: ActionRequiredData;
};

export function getElicitationContent(
  message: Message
): (ActionRequired & { type: 'actionRequired' }) | undefined {
  return message.content.find(
    (content): content is ActionRequired & { type: 'actionRequired' } =>
      content.type === 'actionRequired' && content.data.actionType === 'elicitation'
  );
}
