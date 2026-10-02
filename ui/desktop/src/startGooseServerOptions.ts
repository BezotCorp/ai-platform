import { FindGooseBinaryOptions } from './findGooseBinaryOptions';
import { Logger } from './logger';
import { ReadinessFetch } from './readinessFetch';

export interface StartGooseServeOptions extends FindGooseBinaryOptions {
  dir?: string;
  serverSecret: string;
  tls?: boolean;
  env?: Record<string, string | undefined>;
  /** PATH from the user's login shell, appended so goosed can find CLI providers. */
  loginShellPath?: string | null;
  logger?: Logger;
  diagnosticsDir?: string;
  readinessFetch?: ReadinessFetch;
}
