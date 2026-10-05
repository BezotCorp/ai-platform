import type { PipWindowState } from '../PipWindow/pipWindowState';

export interface PipWindowProps extends PipWindowState {
  title: string;
  /** Omitted when the app does not support fullscreen. */
  onFullscreen?: () => void;
  onClose: () => void;
}
