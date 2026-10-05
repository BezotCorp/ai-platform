import type React from 'react';
import type { GooseDisplayMode } from '../types';
import type { OnDisplayModeChange } from '../types';

export interface UseDisplayModeOptions {
  displayMode: GooseDisplayMode;
  onDisplayModeChange?: OnDisplayModeChange;
  containerRef: React.RefObject<HTMLDivElement | null>;
}
