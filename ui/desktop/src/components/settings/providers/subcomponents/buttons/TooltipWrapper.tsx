import type { TooltipWrapperProps } from './tooltipWrapperProps';

// TooltipWrapper.tsx
import React from 'react';
import { Tooltip, TooltipTrigger, TooltipContent, TooltipProvider } from '../../../../ui/Tooltip';
import { Portal } from '@radix-ui/react-portal';


export function TooltipWrapper({
  children,
  tooltipContent,
  side = 'top',
  align = 'center',
  className = '',
}: TooltipWrapperProps) {
  return (
    <TooltipProvider>
      <Tooltip>
        <TooltipTrigger asChild>{children}</TooltipTrigger>
        <Portal>
          <TooltipContent side={side} align={align} className={className}>
            {typeof tooltipContent === 'string' ? <p>{tooltipContent}</p> : tooltipContent}
          </TooltipContent>
        </Portal>
      </Tooltip>
    </TooltipProvider>
  );
}
