import type { CardBodyProps } from './cardBodyProps';

import React from 'react';


export default function CardBody({ children }: CardBodyProps) {
  return <div className="flex items-center justify-start">{children}</div>;
}
