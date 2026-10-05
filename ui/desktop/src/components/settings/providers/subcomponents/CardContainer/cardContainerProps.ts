import React from 'react';

export interface CardContainerProps {
  header: React.ReactNode;
  body: React.ReactNode;
  onClick: () => void;
  grayedOut: boolean;
  testId?: string;
  borderStyle?: 'solid' | 'dashed';
  className?: string;
}
