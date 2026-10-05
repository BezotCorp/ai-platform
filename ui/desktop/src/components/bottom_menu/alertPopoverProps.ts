import React from 'react';
import { Alert } from '../alerts';

export interface AlertPopoverProps {
  alerts: Alert[];
  children?: React.ReactNode;
}
