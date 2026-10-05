import type { GooseDisplayMode } from './gooseDisplayMode';

/**
 * Callback fired when the display mode changes, either via user-initiated
 * host-side controls or app-initiated `ui/request-display-mode` changes.
 */
export type OnDisplayModeChange = (mode: GooseDisplayMode) => void;
