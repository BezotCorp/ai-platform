import type { ActionRequired } from '../../types/actionRequired';

export type ToolConfirmationData = Extract<ActionRequired['data'], { actionType: 'toolConfirmation' }>;
