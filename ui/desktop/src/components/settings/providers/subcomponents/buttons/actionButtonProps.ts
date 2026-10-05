import React from 'react';
import { Button } from '../../../../ui/button';

export interface ActionButtonProps extends React.ComponentProps<typeof Button> {
  /** Icon component to render, e.g. `RefreshCw` from lucide-react */
  icon?: React.ComponentType<React.SVGProps<globalThis.SVGSVGElement>>;
  /** Tooltip text to show; optional if you want no tooltip. */
  tooltip?: React.ReactNode;
  /** Additional classes for styling. */
  className?: string;
  /** Text to display next to the icon */
  text?: string;
  /** Additional class for the icon specifically */
  iconClassName?: string;
}
