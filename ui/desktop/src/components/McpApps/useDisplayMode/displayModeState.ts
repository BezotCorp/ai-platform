import type { McpUiDisplayMode } from '@modelcontextprotocol/ext-apps/app-bridge';
import type React from 'react';
import type { GooseDisplayMode } from '../types';

export interface DisplayModeState {
  activeDisplayMode: GooseDisplayMode;
  effectiveDisplayModes: McpUiDisplayMode[];
  isStandalone: boolean;
  isFullscreen: boolean;
  isPip: boolean;
  isFillsViewport: boolean;
  isInline: boolean;
  appSupportsFullscreen: boolean;
  appSupportsPip: boolean;
  appTitle: string | null;

  changeDisplayMode: (mode: GooseDisplayMode) => void;

  /** Remembered inline height for placeholders when detached. */
  inlineHeight: number;

  /** Ref for the fullscreen close button (auto-focused on enter). */
  fullscreenCloseRef: React.RefObject<HTMLButtonElement | null>;
}
