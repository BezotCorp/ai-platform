import type React from 'react';
import type { ToolCallStatus } from './toolCallStatus';

/**
 * Wrapper component that adds a status indicator to a tool icon
 */
export interface ToolIconWithStatusProps {
  ToolIcon: React.ComponentType<{ className?: string }>;
  status: ToolCallStatus;
  className?: string;
}
