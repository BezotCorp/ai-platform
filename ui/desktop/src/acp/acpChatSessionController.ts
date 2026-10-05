import type { GooseExtension } from '@aaif/goose-acp-client';
import type { Session } from '../types/session';
import type { Message } from '../types/message';
import type { ImageData } from '../types/imageData';
import { AcpRecipeOptions } from './acpRecipeOptions';
import type { AcpLoadSessionOptions } from './acpLoadSessionOptions';
import type { AcpSubmitMessageOptions } from './acpSubmitMessageOptions';

export interface AcpChatSessionController {
  createSession(
    cwd: string,
    gooseExtensions: GooseExtension[] | undefined,
    recipe?: AcpRecipeOptions
  ): Promise<Session>;
  loadSession(sessionId: string, options?: AcpLoadSessionOptions): Promise<void>;
  restoreSession(sessionId: string): Promise<void>;
  submitMessage(
    sessionId: string,
    userMessage: Message,
    options: AcpSubmitMessageOptions
  ): Promise<void>;
  stop(sessionId: string): void;
  updateMessage(
    sessionId: string,
    messageId: string,
    newContent: string,
    editType: 'fork' | 'edit',
    retainedImages: ImageData[],
    options: AcpSubmitMessageOptions
  ): Promise<void>;
}
