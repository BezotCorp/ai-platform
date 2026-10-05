export interface UsePipWindowOptions {
  /** Whether the app is currently displayed in PiP. */
  active: boolean;
  /** Chat session used to remember size and position between openings. */
  sessionId?: string | null;
}
