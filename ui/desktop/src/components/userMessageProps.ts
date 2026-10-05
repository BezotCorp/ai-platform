import type { Message } from '../types/message';
import type { ImageData } from '../types/imageData';

export interface UserMessageProps {
  message: Message;
  onMessageUpdate?: (
    messageId: string,
    newContent: string,
    editType: 'fork' | 'edit',
    retainedImages: ImageData[]
  ) => void;
}
