import type { View } from './view';
import type { ViewOptions } from './viewOptions';

export type NavigationHandler = (view: View, options?: ViewOptions) => void;
