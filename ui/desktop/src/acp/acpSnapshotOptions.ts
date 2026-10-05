import type { AcpChatSessionSnapshot } from './acpChatSessionSnapshot';


export interface AcpSnapshotOptions {
  getCurrentSnapshot(): AcpChatSessionSnapshot | undefined;
}
