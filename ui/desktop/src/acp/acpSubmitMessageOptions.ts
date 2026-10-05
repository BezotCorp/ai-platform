import type { AcpSnapshotOptions } from './acpSnapshotOptions';

export interface AcpSubmitMessageOptions extends AcpSnapshotOptions {
  onFinish(error?: string): void | Promise<void>;
}
