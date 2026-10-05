import type { PipGeometry } from './pipGeometry';
import type { Viewport } from './viewport';

export type PipGestureApply = (
  origin: PipGeometry,
  dx: number,
  dy: number,
  viewport: Viewport
) => PipGeometry;
