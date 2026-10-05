import type { PipGeometry } from './pipGeometry';
import type { PipResizeHandle } from './pipResizeHandle';
import type { PipGestureHandlers } from './pipGestureHandlers';

export interface PipWindowState {
  /** Size and position offset from the default bottom-right corner. */
  geometry: PipGeometry;
  moveHandlers: PipGestureHandlers;
  /** One set of handlers per edge and corner. */
  resizeHandlers: Record<PipResizeHandle, PipGestureHandlers>;
}
