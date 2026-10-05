/**
 * Per the ext-apps spec, each axis is independently:
 *   fixed     – sends width/height (host controls, view fills it)
 *   flexible  – sends maxWidth/maxHeight (view controls, up to max; host resizes via size-changed)
 *   unbounded – field omitted (view controls with no limit; host resizes via size-changed)
 */
export type DimensionMode = 'fixed' | 'flexible' | 'unbounded';
