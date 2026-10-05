import type { ToolCallStatusIndicatorProps } from './toolCallStatusIndicatorProps';

import React from 'react';
import { defineMessages, useIntl } from '../../i18n';
import { cn } from '../../utils';
import type { MessageValue } from 'react-intl';

import type { ToolCallStatus } from '../ToolCallStatusIndicator/toolCallStatus';
import type { ToolIconWithStatusProps } from '../ToolCallStatusIndicator/toolIconWithStatusProps';
const i18n = defineMessages<{
  readonly "toolStatus": { readonly "status": MessageValue };
}>({
  toolStatus: {
    id: 'toolCallStatusIndicator.toolStatus',
    defaultMessage: 'Tool status: {status}',
  },
});



export const ToolCallStatusIndicator: React.FC<ToolCallStatusIndicatorProps> = ({
  status,
  className,
}) => {
  const intl = useIntl();
  const getStatusStyles = () => {
    switch (status) {
      case 'success':
        return 'bg-green-500';
      case 'error':
        return 'bg-red-500';
      case 'loading':
        return 'bg-yellow-500 animate-pulse';
      case 'pending':
      default:
        return 'bg-gray-400';
    }
  };

  return (
    <div
      className={cn(
        'absolute -top-0.5 -right-0.5 w-2 h-2 rounded-full border border-border-primary',
        getStatusStyles(),
        className
      )}
      aria-label={intl.formatMessage(i18n.toolStatus, { status })}
    />
  );
};


export const ToolIconWithStatus: React.FC<ToolIconWithStatusProps> = ({
  ToolIcon,
  status,
  className,
}) => {
  return (
    <div className={cn('relative inline-block', className)}>
      <ToolIcon className="w-3 h-3 flex-shrink-0" />
      <ToolCallStatusIndicator status={status} />
    </div>
  );
};

export type { ToolCallStatus } from '../ToolCallStatusIndicator/toolCallStatus';
