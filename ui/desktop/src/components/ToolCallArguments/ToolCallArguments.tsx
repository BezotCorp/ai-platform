import type { ToolCallArgumentsProps } from './toolCallArgumentsProps';

import { useState } from 'react';
import Expand from '../ui/Expand';



function formatValue(value: unknown): string {
  if (typeof value === 'string') {
    return value;
  }

  if (
    typeof value === 'number' ||
    typeof value === 'boolean' ||
    typeof value === 'bigint'
  ) {
    return String(value);
  }

  if (value === null) {
    return 'null';
  }

  if (value === undefined) {
    return 'undefined';
  }

  if (typeof value === 'symbol') {
    return value.description ?? 'symbol';
  }

  if (typeof value === 'function') {
    return value.name ? `[function ${value.name}]` : '[function]';
  }

  try {
    return JSON.stringify(value, null, 2) ?? '';
  } catch {
    return '[unserializable value]';
  }
}

export function ToolCallArguments({ args }: ToolCallArgumentsProps) {
  const [expandedKeys, setExpandedKeys] = useState<Record<string, boolean>>({});

  const toggleKey = (key: string) => {
    setExpandedKeys((prev) => ({ ...prev, [key]: !prev[key] }));
  };

  const renderValue = (key: string, value: unknown) => {
    const text = formatValue(value).trim();
    const needsExpansion = text.length > 60 || text.includes('\n');
    const isExpanded = expandedKeys[key];

    return (
      <div className="font-sans text-sm mb-2">
        <div className={`flex flex-row items-stretch ${!isExpanded && needsExpansion ? 'truncate min-w-0' : ''}`}>
          <button
            onClick={() => needsExpansion && toggleKey(key)}
            className={`flex text-left text-text-secondary min-w-[140px] ${needsExpansion ? 'cursor-pointer' : 'cursor-default'}`}
          >
            <span>{key}</span>
          </button>
          <div className={`w-full flex items-stretch ${!isExpanded && needsExpansion ? 'truncate min-w-0' : ''}`}>
            {isExpanded ? (
              <pre className="font-mono text-xs text-text-secondary whitespace-pre-wrap max-w-full overflow-x-auto">
                {text}
              </pre>
            ) : (
              <button
                onClick={() => needsExpansion && toggleKey(key)}
                className={`text-left text-text-secondary font-mono text-xs ${needsExpansion ? 'truncate min-w-0 cursor-pointer' : 'cursor-default'}`}
              >
                {text.split('\n')[0]}
              </button>
            )}
            {needsExpansion && (
              <button
                onClick={() => toggleKey(key)}
                className="flex flex-row items-stretch grow text-text-secondary pr-2"
              >
                <div className="min-w-2 grow" />
                <Expand size={5} isExpanded={isExpanded} />
              </button>
            )}
          </div>
        </div>
      </div>
    );
  };

  return (
    <div className="my-2">
      {Object.entries(args).map(([key, value]) => (
        <div key={key}>{renderValue(key, value)}</div>
      ))}
    </div>
  );
}

export type { ToolCallArgumentValue } from '../ToolCallArguments/toolCallArgumentValue';
