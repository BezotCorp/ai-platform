import type React from 'react';

export interface IconInfo {
  Icon: React.ComponentType<{ className?: string; style?: React.CSSProperties }>;
  color: string;
}
