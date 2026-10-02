import { AppEvents } from '../constants/events';
import { ToolIconWithStatus, ToolCallStatus } from './ToolCallStatusIndicator';
import { getToolCallIcon } from '../utils/toolIconMapping';
import React, { useEffect, useRef, useState } from 'react';
import { Button } from './ui/button';
import { ToolCallArguments } from './ToolCallArguments';
import MarkdownContent from './MarkdownContent';
import {
  ToolRequestMessageContent,
  ToolResponseMessageContent,
  NotificationEvent,
  ToolConfirmationData,
} from '../types/message';
import { cn, snakeToTitleCase } from '../utils';
import { ChevronRight, ExternalLink } from 'lucide-react';
import { TooltipWrapper } from './settings/providers/subcomponents/buttons/TooltipWrapper';
import type { CallToolResult } from '@modelcontextprotocol/sdk/types.js';
import type { ContentBlock } from '../types/message';

import McpAppRenderer from './McpApps/McpAppRenderer';
import ToolApprovalButtons from './ToolApprovalButtons';
import { defineMessages, useIntl } from '../i18n';
import type { MessageValue, NoMessageValues } from 'react-intl';

type LoadingStatus = 'loading' | 'success' | 'error';

const i18n = defineMessages<{
  readonly "viewSubagentSession": NoMessageValues;
  readonly "toolDetails": NoMessageValues;
  readonly "code": NoMessageValues;
  readonly "output": NoMessageValues;
  readonly "toolResultAlt": NoMessageValues;
  readonly "activityCount": { readonly "count": MessageValue };
  readonly "logs": NoMessageValues;
  readonly "loadingSpinner": NoMessageValues;
}>({
  viewSubagentSession: {
    id: 'toolCallWithResponse.viewSubagentSession',
    defaultMessage: 'View subagent session',
  },
  toolDetails: {
    id: 'toolCallWithResponse.toolDetails',
    defaultMessage: 'Tool Details',
  },
  code: {
    id: 'toolCallWithResponse.code',
    defaultMessage: 'Code',
  },
  output: {
    id: 'toolCallWithResponse.output',
    defaultMessage: 'Output',
  },
  toolResultAlt: {
    id: 'toolCallWithResponse.toolResultAlt',
    defaultMessage: 'Tool result',
  },
  activityCount: {
    id: 'toolCallWithResponse.activityCount',
    defaultMessage: 'Activity ({count})',
  },
  logs: {
    id: 'toolCallWithResponse.logs',
    defaultMessage: 'Logs',
  },
  loadingSpinner: {
    id: 'toolCallWithResponse.loadingSpinner',
    defaultMessage: 'Loading spinner',
  },
});

interface ToolGraphNode {
  tool: string;
  description: string;
  depends_on: number[];
}

const isRecord = (value: unknown): value is Record<string, unknown> =>
  typeof value === 'object' && value !== null && !Array.isArray(value);

const isToolGraphNode = (value: unknown): value is ToolGraphNode => {
  if (!isRecord(value)) {
    return false;
  }

  return (
    typeof value.tool === 'string' &&
    typeof value.description === 'string' &&
    Array.isArray(value.depends_on) &&
    value.depends_on.every((dependency) => typeof dependency === 'number')
  );
};

const getToolGraph = (value: unknown): ToolGraphNode[] | undefined =>
  Array.isArray(value) && value.every(isToolGraphNode) ? value : undefined;

interface ParsedToolCall {
  name: string;
  arguments: Record<string, unknown>;
}

function getParsedToolCall(value: unknown): ParsedToolCall | null {
  if (!isRecord(value)) {
    return null;
  }

  const candidate = value.status === 'success' ? value.value : value;
  if (!isRecord(candidate) || typeof candidate.name !== 'string') {
    return null;
  }

  return {
    name: candidate.name,
    arguments: isRecord(candidate.arguments) ? candidate.arguments : {},
  };
}

function getToolResultUiMeta(value: unknown): UiMeta | undefined {
  if (!isRecord(value) || value.status !== 'success' || !isRecord(value.value)) {
    return undefined;
  }

  const meta = value.value._meta;
  if (!isRecord(meta)) {
    return undefined;
  }

  const uiValue = meta.ui;
  const ui =
    isRecord(uiValue) && typeof uiValue.resourceUri === 'string'
      ? { resourceUri: uiValue.resourceUri }
      : undefined;

  return {
    ui,
    extensionName:
      typeof meta.extensionName === 'string' ? meta.extensionName : undefined,
    toolName: typeof meta.toolName === 'string' ? meta.toolName : undefined,
    toolNameIsActual:
      typeof meta.toolNameIsActual === 'boolean'
        ? meta.toolNameIsActual
        : undefined,
    subagent_session_id:
      typeof meta.subagent_session_id === 'string'
        ? meta.subagent_session_id
        : undefined,
  };
}

function isProgress(value: unknown): value is Progress {
  if (!isRecord(value)) {
    return false;
  }

  return (
    typeof value.progress === 'number' &&
    typeof value.progressToken === 'string' &&
    (value.total === undefined || typeof value.total === 'number') &&
    (value.message === undefined || typeof value.message === 'string')
  );
}


type UiMeta = {
  ui?: {
    resourceUri?: string;
  };
  extensionName?: string;
  toolName?: string;
  toolNameIsActual?: boolean;
  subagent_session_id?: string;
};

type ToolResultValue = {
  content: ContentBlock[];
  structuredContent?: unknown;
  isError: boolean;
  _meta?: UiMeta;
};

interface ToolCallWithResponseProps {
  sessionId?: string;
  isCancelledMessage: boolean;
  toolRequest: ToolRequestMessageContent;
  toolResponse?: ToolResponseMessageContent;
  notifications?: NotificationEvent[];
  isStreamingMessage?: boolean;
  isPendingApproval: boolean;
  append?: (value: string) => void;
  confirmationContent?: ToolConfirmationData;
  isApprovalClicked?: boolean;
}

function getSubagentSessionId(
  toolResponse?: ToolResponseMessageContent,
  notifications?: NotificationEvent[]
): string | null {
  const sessionId = getToolResultUiMeta(toolResponse?.toolResult)?.subagent_session_id;
  if (sessionId) {
    return sessionId;
  }

  // Fallback: extract from subagent notifications (e.g. when delegate was cancelled mid-stream)
  if (notifications) {
    for (const notification of notifications) {
      if (notification.message.method !== 'notifications/message') {
        continue;
      }

      const params = notification.message.params;
      if (!isRecord(params)) {
        continue;
      }

      const data = params.data;
      if (
        isRecord(data) &&
        data.type === 'subagent_tool_request' &&
        typeof data.subagent_id === 'string'
      ) {
        return data.subagent_id;
      }
    }
  }

  return null;
}

function getToolResultContent(toolResult: Record<string, unknown>): ContentBlock[] {
  if (toolResult.status === 'error') {
    return typeof toolResult.error === 'string' ? [{ type: 'text', text: toolResult.error }] : [];
  }
  if (toolResult.status !== 'success') {
    return [];
  }
  const value = toolResult.value as ToolResultValue;
  return value.content.filter((item) => {
    const annotations = (item as { annotations?: { audience?: string[] } }).annotations;
    return !annotations?.audience || annotations.audience.includes('user');
  });
}

interface McpAppWrapperProps {
  toolRequest: ToolRequestMessageContent;
  toolResponse?: ToolResponseMessageContent;
  sessionId: string;
  append?: (value: string) => void;
}

export function resolveMcpAppMetadata(
  responseMeta: UiMeta | undefined
): { resourceUri: string; extensionName: string; toolName: string } | null {
  const resourceUri = responseMeta?.ui?.resourceUri;
  const extensionName = responseMeta?.extensionName;
  const toolName = responseMeta?.toolName;
  if (resourceUri && extensionName && toolName) {
    const legacyPrefix = `${extensionName}__`;
    const actualToolName = responseMeta.toolNameIsActual
      ? toolName
      : toolName.startsWith(legacyPrefix)
        ? toolName.slice(legacyPrefix.length)
        : toolName;
    if (actualToolName) {
      return { resourceUri, extensionName, toolName: actualToolName };
    }
  }

  return null;
}

function McpAppWrapper({
  toolRequest,
  toolResponse,
  sessionId,
  append,
}: McpAppWrapperProps): React.ReactNode {
  const parsedToolCall = getParsedToolCall(toolRequest.toolCall);
  const responseMeta = getToolResultUiMeta(toolResponse?.toolResult);
  const appMetadata = resolveMcpAppMetadata(responseMeta);

  if (!appMetadata || !parsedToolCall) {
    return null;
  }

  const toolInput = { arguments: parsedToolCall.arguments };

  const rawToolResult = toolResponse?.toolResult;
  const toolResult =
    isRecord(rawToolResult) &&
    rawToolResult.status === 'success' &&
    isRecord(rawToolResult.value) &&
    Array.isArray(rawToolResult.value.content)
      ? ({
          content: rawToolResult.value.content,
          structuredContent: isRecord(rawToolResult.value.structuredContent)
            ? rawToolResult.value.structuredContent
            : undefined,
          isError:
            typeof rawToolResult.value.isError === 'boolean'
              ? rawToolResult.value.isError
              : undefined,
        } satisfies CallToolResult)
      : undefined;

  const { resourceUri, extensionName, toolName } = appMetadata;

  return (
    <div className="mt-3">
      <McpAppRenderer
        resourceUri={resourceUri}
        toolInput={toolInput}
        toolResult={toolResult}
        extensionName={extensionName}
        toolName={toolName}
        sessionId={sessionId}
        append={append}
      />
    </div>
  );
}

export default function ToolCallWithResponse({
  sessionId,
  isCancelledMessage,
  toolRequest,
  toolResponse,
  notifications,
  isStreamingMessage,
  isPendingApproval,
  append,
  confirmationContent,
  isApprovalClicked,
}: ToolCallWithResponseProps) {
  // Handle both the wrapped ToolResult format and the unwrapped format
  // The server serializes ToolResult<T> as { status: "success", value: T } or { status: "error", error: string }
  const toolCall = getParsedToolCall(toolRequest.toolCall);

  if (!toolCall) {
    return null;
  }

  const responseMeta = getToolResultUiMeta(toolResponse?.toolResult);
  const hasMcpAppResourceURI = Boolean(responseMeta?.ui?.resourceUri);

  const shouldShowMcpContent = !isPendingApproval;

  const showInlineApproval = isPendingApproval && confirmationContent && sessionId;

  return (
    <>
      <div
        className={cn(
          'w-full text-sm font-sans rounded-lg overflow-hidden border',
          showInlineApproval ? 'border-amber-500/50 bg-amber-50/5' : 'border-border-primary'
        )}
      >
        <ToolCallView
          {...{
            isCancelledMessage,
            toolCall,
            toolResponse,
            notifications,
            isStreamingMessage,
          }}
        />
        {/* Inline approval UI */}
        {showInlineApproval && (
          <div className="border-t border-amber-500/30">
            {confirmationContent.prompt && (
              <div className="px-4 py-2 text-sm text-amber-600 dark:text-amber-400 bg-amber-50/10">
                {confirmationContent.prompt}
              </div>
            )}
            <div className="px-4 pb-2">
              <ToolApprovalButtons
                data={{
                  generation: confirmationContent.generation,
                  id: confirmationContent.id,
                  toolName: confirmationContent.toolName,
                  prompt: confirmationContent.prompt ?? undefined,
                  sessionId,
                  isClicked: isApprovalClicked,
                }}
              />
            </div>
          </div>
        )}
      </div>

      {/* MCP App */}
      {shouldShowMcpContent && hasMcpAppResourceURI && sessionId && (
        <McpAppWrapper
          toolRequest={toolRequest}
          toolResponse={toolResponse}
          sessionId={sessionId}
          append={append}
        />
      )}
    </>
  );
}

interface ToolCallExpandableProps {
  label: string | React.ReactNode;
  isStartExpanded?: boolean;
  isForceExpand?: boolean;
  children: React.ReactNode;
  className?: string;
}

function ToolCallExpandable({
  label,
  isStartExpanded = false,
  isForceExpand,
  children,
  className = '',
}: ToolCallExpandableProps) {
  const [isExpandedState, setIsExpanded] = React.useState<boolean | null>(null);
  const isExpanded = isExpandedState === null ? isStartExpanded : isExpandedState;
  const toggleExpand = () => setIsExpanded(!isExpanded);
  React.useEffect(() => {
    if (isForceExpand) setIsExpanded(true);
  }, [isForceExpand]);

  return (
    <div className={className}>
      <Button
        onClick={toggleExpand}
        className="group w-full flex justify-between items-center pr-2 transition-colors rounded-none"
        variant="ghost"
      >
        <span className="flex items-center font-sans text-sm truncate flex-1 min-w-0">{label}</span>
        <ChevronRight
          className={cn(
            'group-hover:opacity-100 transition-transform opacity-70',
            isExpanded && 'rotate-90'
          )}
        />
      </Button>
      {isExpanded && <div>{children}</div>}
    </div>
  );
}

interface ToolCallViewProps {
  isCancelledMessage: boolean;
  toolCall: {
    name: string;
    arguments: Record<string, unknown>;
  };
  toolResponse?: ToolResponseMessageContent;
  notifications?: NotificationEvent[];
  isStreamingMessage?: boolean;
}

interface Progress {
  progress: number;
  progressToken: string;
  total?: number;
  message?: string;
}

interface SubagentToolRequestData {
  type: 'subagent_tool_request';
  subagent_id: string;
  tool_call: {
    name: string;
    arguments?: { tool_graph?: ToolGraphNode[] };
  };
}

const isSubagentToolRequestData = (data: unknown): data is SubagentToolRequestData => {
  if (
    !isRecord(data) ||
    data.type !== 'subagent_tool_request' ||
    typeof data.subagent_id !== 'string' ||
    !isRecord(data.tool_call) ||
    typeof data.tool_call.name !== 'string'
  ) {
    return false;
  }

  const argumentsValue = data.tool_call.arguments;
  if (argumentsValue === undefined) {
    return true;
  }

  if (!isRecord(argumentsValue)) {
    return false;
  }

  return (
    argumentsValue.tool_graph === undefined ||
    getToolGraph(argumentsValue.tool_graph) !== undefined
  );
};

const formatSubagentToolCall = (data: SubagentToolRequestData): string => {
  const subagentId = data.subagent_id;
  const toolCall = data.tool_call;
  const toolCallName = toolCall.name;

  const shortId = subagentId?.split('_').pop() || subagentId;

  const parts = toolCallName.split('__').reverse();
  const toolName = parts[0] || 'unknown';
  const extensionName = parts.slice(1).reverse().join('__') || '';
  const toolGraph = toolCall.arguments?.tool_graph;

  if (toolName === 'execute_typescript' && toolGraph && toolGraph.length > 0) {
    const plural = toolGraph.length === 1 ? '' : 's';
    const header = `[subagent:${shortId}] ${toolGraph.length} tool call${plural} | execute_typescript`;
    const lines = toolGraph.map((node, idx) => {
      const deps =
        node.depends_on && node.depends_on.length > 0
          ? ` (uses ${node.depends_on.map((d) => d + 1).join(', ')})`
          : '';
      return `  ${idx + 1}. ${node.tool}: ${node.description}${deps}`;
    });
    return [header, ...lines].join('\n');
  }

  return extensionName
    ? `[subagent:${shortId}] ${toolName} | ${extensionName}`
    : `[subagent:${shortId}] ${toolName}`;
};

const logToString = (logMessage: NotificationEvent): string => {
  const params = isRecord(logMessage.message.params)
    ? logMessage.message.params
    : {};
  const data = params.data;

  if (isSubagentToolRequestData(data)) {
    return formatSubagentToolCall(data);
  }

  if (isRecord(data)) {
    const stream = data.stream;
    const output = data.output;

    if (
      (typeof stream === 'string' || typeof stream === 'number') &&
      (typeof output === 'string' || typeof output === 'number')
    ) {
      return `[${stream}] ${output}`;
    }
  }

  if (typeof data === 'string') {
    return data;
  }

  try {
    return JSON.stringify(data) ?? '';
  } catch {
    return '[unserializable log data]';
  }
};

const notificationToProgress = (notification: NotificationEvent): Progress | null =>
  isProgress(notification.message.params)
    ? notification.message.params
    : null;

const liveOutputToString = (notifications: NotificationEvent[] | undefined): string =>
  notifications
    ?.filter((notification) => notification.message.method === 'goose/live_output')
    .flatMap((notification) => {
      const params = notification.message.params;
      if (!isRecord(params) || !Array.isArray(params.chunks)) {
        return [];
      }

      return params.chunks.flatMap((chunk) =>
        isRecord(chunk) && typeof chunk.output === 'string'
          ? [chunk.output]
          : []
      );
    })
    .join('') ?? '';

// Helper function to extract toolcall name
const getToolName = (toolCallName: string): string => {
  const lastIndex = toolCallName.lastIndexOf('__');
  if (lastIndex === -1) return toolCallName;

  return toolCallName.substring(lastIndex + 2);
};

// Helper function to extract extension name for tooltip
const getExtensionTooltip = (toolCallName: string): string | null => {
  const lastIndex = toolCallName.lastIndexOf('__');
  if (lastIndex === -1) return null;

  const extensionName = toolCallName.substring(0, lastIndex);
  if (!extensionName) return null;

  return `${extensionName} extension`;
};

function ToolCallView({
  isCancelledMessage,
  toolCall,
  toolResponse,
  notifications,
  isStreamingMessage = false,
}: ToolCallViewProps) {
  const intl = useIntl();
  const [responseStyle, setResponseStyle] = useState<string>('concise');

  useEffect(() => {
    // Load initial value from settings
    void window.electron
      .getSetting('responseStyle')
      .then(setResponseStyle)
      .catch((error) => {
        console.error('Failed to read response style:', error);
      });

    const handleStyleChange = () => {
      void window.electron
        .getSetting('responseStyle')
        .then(setResponseStyle)
        .catch((error) => {
          console.error('Failed to refresh response style:', error);
        });
    };

    window.addEventListener(AppEvents.RESPONSE_STYLE_CHANGED, handleStyleChange);

    return () => {
      window.removeEventListener(AppEvents.RESPONSE_STYLE_CHANGED, handleStyleChange);
    };
  }, []);

  const isExpandToolDetails = (() => {
    switch (responseStyle) {
      case 'concise':
        return false;
      case 'detailed':
      default:
        return true;
    }
  })();

  const isToolDetails = toolCall?.arguments && Object.entries(toolCall.arguments).length > 0;

  // Check if streaming has finished but no tool response was received
  // This is a workaround for cases where the backend doesn't send tool responses
  const isStreamingComplete = !isStreamingMessage;
  const shouldShowAsComplete = isStreamingComplete && !toolResponse;
  const toolResult = toolResponse?.toolResult;
  const toolResultValue =
    toolResult && 'value' in toolResult && isRecord(toolResult.value)
      ? toolResult.value
      : undefined;
  const toolResultIsError = toolResultValue?.isError === true;

  const loadingStatus: LoadingStatus = !toolResponse
    ? shouldShowAsComplete
      ? 'success'
      : 'loading'
    : toolResult?.status === 'error' || toolResultIsError
      ? 'error'
      : 'success';

  // Tool call timing tracking
  const [startTime, setStartTime] = useState<number | null>(null);

  // Track when tool call starts (when there's no response yet)
  useEffect(() => {
    if (!toolResponse && startTime === null) {
      setStartTime(Date.now());
    }
  }, [toolResponse, startTime]);

  const toolResults = toolResult ? getToolResultContent(toolResult) : [];
  const liveOutput = toolResponse ? '' : liveOutputToString(notifications);

  const logs = notifications
    ?.filter(
      (notification) => notification.message.method === 'notifications/message'
    )
    .map(logToString);

  const progress = notifications
    ?.filter(
      (notification) => notification.message.method === 'notifications/progress'
    )
    .map(notificationToProgress)
    .filter((item): item is Progress => item !== null)
    .reduce((map, item) => {
      const key = item.progressToken;
      if (!map.has(key)) {
        map.set(key, []);
      }
      map.get(key)!.push(item);
      return map;
    }, new Map<string, Progress[]>());

  const progressEntries = [...(progress?.values() || [])].map(
    (entries) => entries.sort((a, b) => b.progress - a.progress)[0]
  );

  const isRenderingActivity =
    loadingStatus === 'loading' &&
    (progressEntries.length > 0 || (logs || []).length > 0 || liveOutput.length > 0);

  // Function to create a descriptive representation of what the tool is doing
  const getToolDescription = (): string | null => {
    const args = toolCall.arguments ?? {};
    const toolName = getToolName(toolCall.name);

    const getStringValue = (value: unknown): string => {
      if (typeof value === 'string') {
        return value;
      }

      try {
        return JSON.stringify(value) ?? '';
      } catch {
        return '[unserializable value]';
      }
    };

    // Generate descriptive text based on tool type
    switch (toolName) {
      case 'text_editor':
        if (args.command === 'write' && args.path) {
          return `writing ${getStringValue(args.path)}`;
        }
        if (args.command === 'view' && args.path) {
          return `reading ${getStringValue(args.path)}`;
        }
        if (args.command === 'str_replace' && args.path) {
          return `editing ${getStringValue(args.path)}`;
        }
        if (args.command && args.path) {
          return `${getStringValue(args.command)} ${getStringValue(args.path)}`;
        }
        break;

      case 'shell':
        if (args.command) {
          return `running ${getStringValue(args.command)}`;
        }
        break;

      case 'search':
        if (args.name) {
          return `searching for "${getStringValue(args.name)}"`;
        }
        if (args.mimeType) {
          return `searching for ${getStringValue(args.mimeType)} files`;
        }
        break;

      case 'read': {
        if (args.uri) {
          const uri = getStringValue(args.uri);
          const fileId = uri.replace('gdrive:///', '');
          return `reading file ${fileId}`;
        }
        if (args.url) {
          return `reading ${getStringValue(args.url)}`;
        }
        break;
      }

      case 'create_file':
        if (args.name) {
          return `creating ${getStringValue(args.name)}`;
        }
        break;

      case 'update_file':
        if (args.fileId) {
          return `updating file ${getStringValue(args.fileId)}`;
        }
        break;

      case 'sheets_tool': {
        if (args.operation && args.spreadsheetId) {
          const operation = getStringValue(args.operation);
          const sheetId = getStringValue(args.spreadsheetId);
          return `${operation} in sheet ${sheetId}`;
        }
        break;
      }

      case 'docs_tool': {
        if (args.operation && args.documentId) {
          const operation = getStringValue(args.operation);
          const docId = getStringValue(args.documentId);
          return `${operation} in document ${docId}`;
        }
        break;
      }

      case 'remember_memory':
        if (args.category && args.data) {
          return `storing ${getStringValue(args.category)}: ${getStringValue(args.data)}`;
        }
        break;

      case 'retrieve_memories':
        if (args.category) {
          return `retrieving ${getStringValue(args.category)} memories`;
        }
        break;

      case 'screen_capture':
        if (args.window_title) {
          return `capturing window "${getStringValue(args.window_title)}"`;
        }
        return `capturing screen`;

      case 'delegate': {
        if (args.instructions) {
          const instr = getStringValue(args.instructions);
          const truncated = instr.length > 80 ? instr.substring(0, 80) + '…' : instr;
          return `delegating: ${truncated}`;
        }
        if (args.source) {
          return `delegating to ${getStringValue(args.source)}`;
        }
        return 'delegating task';
      }

      case 'load': {
        if (args.source) {
          return `loading ${getStringValue(args.source)}`;
        }
        return 'loading source';
      }

      case 'final_output':
        return 'final output';

      case 'computer_control':
        return `poking around...`;

      case 'execute_typescript': {
        const toolGraph = getToolGraph(args.tool_graph);
        if (toolGraph && toolGraph.length > 0) {
          if (toolGraph.length === 1) {
            return toolGraph[0].description;
          }
          if (toolGraph.length === 2) {
            return `${toolGraph[0].tool}, ${toolGraph[1].tool}`;
          }
          return `${toolGraph.length} tools used`;
        }
        return 'executing code';
      }

      default: {
        // Generic fallback for unknown tools: ToolName + CompactArguments
        // This ensures any MCP tool works without explicit handling
        const toolDisplayName = snakeToTitleCase(toolName);
        const entries = Object.entries(args);

        if (entries.length === 0) {
          return toolDisplayName;
        }

        // For a single parameter, show key and truncated value
        if (entries.length === 1) {
          const [key, value] = entries[0];
          const stringValue = getStringValue(value);
          return `${toolDisplayName} ${key}: ${stringValue}`;
        }

        // For multiple parameters, show tool name and keys
        const keys = entries.map(([key]) => key).join(', ');
        return `${toolDisplayName} ${keys}`;
      }
    }

    return null;
  };

  // Get extension tooltip for the current tool
  const extensionTooltip = getExtensionTooltip(toolCall.name);

  // Extract tool label content to avoid duplication
  const getToolLabelContent = () => {
    const description = getToolDescription();
    if (description) {
      return description;
    }
    // Fallback tool name formatting
    return snakeToTitleCase(getToolName(toolCall.name));
  };
  // Map LoadingStatus to ToolCallStatus
  const getToolCallStatus = (loadingStatus: LoadingStatus): ToolCallStatus => {
    switch (loadingStatus) {
      case 'success':
        return 'success';
      case 'error':
        return 'error';
      case 'loading':
        return 'loading';
      default:
        return 'pending';
    }
  };

  const toolCallStatus = getToolCallStatus(loadingStatus);

  const toolLabel = (
    <span
      className={cn(
        'flex items-center gap-2 min-w-0',
        extensionTooltip && 'cursor-pointer hover:opacity-80'
      )}
    >
      <ToolIconWithStatus ToolIcon={getToolCallIcon(toolCall.name)} status={toolCallStatus} />
      <span className="truncate flex-1 min-w-0">{getToolLabelContent()}</span>
    </span>
  );
  return (
    <ToolCallExpandable
      isStartExpanded={isRenderingActivity || isExpandToolDetails}
      isForceExpand={false}
      label={
        extensionTooltip ? (
          <TooltipWrapper tooltipContent={extensionTooltip} side="top" align="start">
            {toolLabel}
          </TooltipWrapper>
        ) : (
          toolLabel
        )
      }
    >
      {(() => {
        const rawCode = toolCall.arguments?.code;
        const code = typeof rawCode === 'string' ? rawCode : undefined;
        const toolGraph = getToolGraph(toolCall.arguments?.tool_graph);

        if (
          toolCall.name === 'code_execution__execute_typescript' &&
          (code !== undefined || toolGraph !== undefined)
        ) {
          return (
            <div className="border-t border-border-primary">
              <CodeModeView toolGraph={toolGraph} code={code} />
            </div>
          );
        }

        if (isToolDetails) {
          return (
            <div className="border-t border-border-primary">
              <ToolDetailsView toolCall={toolCall} isStartExpanded={isExpandToolDetails} />
            </div>
          );
        }

        return null;
      })()}

      {logs && logs.length > 0 && (
        <div className="border-t border-border-primary">
          <ToolLogsView
            logs={logs}
            working={loadingStatus === 'loading'}
            isStartExpanded={
              loadingStatus === 'loading' || responseStyle === 'detailed' || responseStyle === null
            }
          />
        </div>
      )}

      {liveOutput && (
        <div className="border-t border-border-primary">
          <LiveOutputView output={liveOutput} />
        </div>
      )}

      {toolResults.length === 0 &&
        progressEntries.length > 0 &&
        progressEntries.map((entry, index) => (
          <div className="p-3 border-t border-border-primary" key={index}>
            <ProgressBar progress={entry.progress} total={entry.total} message={entry.message} />
          </div>
        ))}

      {/* Tool Output */}
      {!isCancelledMessage && (
        <>
          {toolResults.map((result, index) => (
            <div key={index} className={cn('border-t border-border-primary')}>
              <ToolResultView
                toolCall={toolCall}
                result={result}
                isStartExpanded={isExpandToolDetails}
              />
            </div>
          ))}
        </>
      )}

      {(() => {
        if (loadingStatus === 'loading') return null;
        const subagentSessionId = getSubagentSessionId(toolResponse, notifications);
        if (!subagentSessionId) return null;
        return (
          <div className="border-t border-border-primary">
            <button
              onClick={() => {
                window.electron.createChatWindow({
                  resumeSessionId: subagentSessionId,
                  viewType: 'pair',
                });
              }}
              className="w-full flex items-center gap-2 px-4 py-2 text-xs text-text-secondary hover:text-text-primary hover:bg-background-secondary transition-colors cursor-pointer"
            >
              <ExternalLink className="w-3 h-3 flex-shrink-0" />
              <span>{intl.formatMessage(i18n.viewSubagentSession)}</span>
            </button>
          </div>
        );
      })()}
    </ToolCallExpandable>
  );
}

interface ToolDetailsViewProps {
  toolCall: {
    name: string;
    arguments: Record<string, unknown>;
  };
  isStartExpanded: boolean;
}

function ToolDetailsView({ toolCall, isStartExpanded }: ToolDetailsViewProps) {
  const intl = useIntl();
  return (
    <ToolCallExpandable
      label={<span className="pl-4 font-sans text-sm">{intl.formatMessage(i18n.toolDetails)}</span>}
      isStartExpanded={isStartExpanded}
    >
      <div className="pr-4 pl-8">
        {toolCall.arguments && (
          <ToolCallArguments args={toolCall.arguments} />
        )}
      </div>
    </ToolCallExpandable>
  );
}

interface CodeModeViewProps {
  toolGraph?: ToolGraphNode[];
  code?: string;
}

function CodeModeView({ toolGraph, code }: CodeModeViewProps) {
  const intl = useIntl();
  const renderGraph = () => {
    const graph = toolGraph ?? [];
    if (graph.length === 0) return null;

    const lines: string[] = [];

    graph.forEach((node, index) => {
      const deps =
        node.depends_on.length > 0 ? ` (uses ${node.depends_on.map((d) => d + 1).join(', ')})` : '';
      lines.push(`${index + 1}. ${node.tool}: ${node.description}${deps}`);
    });

    return lines.join('\n');
  };

  return (
    <div className="px-4 py-2">
      {toolGraph && (
        <pre className="font-mono text-xs text-textSubtle whitespace-pre-wrap">{renderGraph()}</pre>
      )}
      {code && (
        <div className="border-t border-border-primary -mx-4 mt-2">
          <ToolCallExpandable
            label={<span className="pl-4 font-sans text-sm">{intl.formatMessage(i18n.code)}</span>}
            isStartExpanded={false}
          >
            <MarkdownContent
              content={'```typescript\n' + code + '\n```'}
              className="whitespace-pre-wrap max-w-full overflow-x-auto"
            />
          </ToolCallExpandable>
        </div>
      )}
    </div>
  );
}

interface ToolResultViewProps {
  toolCall: {
    name: string;
    arguments: Record<string, unknown>;
  };
  result: ContentBlock;
  isStartExpanded: boolean;
}

function LiveOutputView({ output }: { output: string }) {
  const intl = useIntl();
  const outputRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (outputRef.current) {
      outputRef.current.scrollTop = outputRef.current.scrollHeight;
    }
  }, [output]);

  return (
    <ToolCallExpandable
      label={<span className="pl-4 py-1 font-sans text-sm">{intl.formatMessage(i18n.output)}</span>}
      isStartExpanded={true}
    >
      <div ref={outputRef} className="max-h-[20rem] overflow-y-auto px-4 py-3">
        <pre className="font-mono text-xs text-textSubtle whitespace-pre-wrap break-words">
          {output}
        </pre>
      </div>
    </ToolCallExpandable>
  );
}

function ToolResultView({ result, isStartExpanded }: ToolResultViewProps) {
  const intl = useIntl();
  const hasText = (c: ContentBlock): c is ContentBlock & { text: string } =>
    'text' in c && typeof c.text === 'string';

  const hasImage = (c: ContentBlock): c is ContentBlock & { data: string; mimeType: string } => {
    if (!('data' in c && 'mimeType' in c)) return false;
    const mimeType = c.mimeType;
    return typeof mimeType === 'string' && mimeType.startsWith('image');
  };

  const hasResource = (c: ContentBlock): c is ContentBlock & { resource: unknown } =>
    'resource' in c;

  return (
    <ToolCallExpandable
      label={<span className="pl-4 py-1 font-sans text-sm">{intl.formatMessage(i18n.output)}</span>}
      isStartExpanded={isStartExpanded}
    >
      <div className="pl-4 pr-4 py-4">
        {hasText(result) && (
          <pre className="font-mono text-xs whitespace-pre-wrap max-w-full overflow-x-auto">
            {result.text.trim()}
          </pre>
        )}
        {hasImage(result) && (
          <img
            src={`data:${result.mimeType};base64,${result.data}`}
            alt={intl.formatMessage(i18n.toolResultAlt)}
            className="max-w-full h-auto rounded-md my-2"
            onError={(e) => {
              console.error('Failed to load image');
              e.currentTarget.style.display = 'none';
            }}
          />
        )}
        {hasResource(result) && (
          <pre className="font-sans text-sm">{JSON.stringify(result, null, 2)}</pre>
        )}
      </div>
    </ToolCallExpandable>
  );
}

function SubagentLogEntry({ log }: { log: string }) {
  const subagentMatch = log.match(/^\[subagent:(\w+)\]\s*([\s\S]*)/);
  if (!subagentMatch) {
    return <span className="font-sans text-sm text-textSubtle">{log}</span>;
  }

  const [, , rest] = subagentMatch;
  const [firstLine, ...detailLines] = rest.split('\n');
  const parts = firstLine.split(' | ');
  const toolName = parts[0]?.trim() || firstLine;
  const extensionName = parts[1]?.trim();

  return (
    <div className="font-sans text-sm text-textSubtle">
      <span className="flex items-center gap-1.5">
        <span className="inline-block w-1.5 h-1.5 rounded-full bg-blue-400 flex-shrink-0" />
        <span className="font-medium text-text-secondary">{toolName}</span>
        {extensionName && <span className="text-textSubtle opacity-60">· {extensionName}</span>}
      </span>
      {detailLines.length > 0 && (
        <pre className="ml-3 mt-0.5 text-xs text-textSubtle whitespace-pre-wrap">
          {detailLines.join('\n')}
        </pre>
      )}
    </div>
  );
}

function ToolLogsView({
  logs,
  working,
  isStartExpanded,
}: {
  logs: string[];
  working: boolean;
  isStartExpanded?: boolean;
}) {
  const intl = useIntl();
  const boxRef = useRef<HTMLDivElement>(null);

  // Whenever logs update, jump to the newest entry
  useEffect(() => {
    if (boxRef.current) {
      boxRef.current.scrollTop = boxRef.current.scrollHeight;
    }
  }, [logs.length]);
  // normally we do not want to put .length on an array in react deps:
  //
  // if the objects inside the array change but length doesn't change you want updates
  //
  // in this case, this is array of strings which once added do not change so this cuts
  // down on the possibility of unwanted runs

  const subagentLogCount = logs.filter((l) => l.startsWith('[subagent:')).length;
  const labelText =
    subagentLogCount > 0
      ? intl.formatMessage(i18n.activityCount, { count: subagentLogCount })
      : intl.formatMessage(i18n.logs);

  return (
    <ToolCallExpandable
      label={
        <span className="pl-4 py-1 font-sans text-sm flex items-center">
          <span>{labelText}</span>
          {working && (
            <div className="mx-2 inline-block">
              <span
                className="inline-block animate-spin rounded-full border-2 border-t-transparent border-current"
                style={{ width: 8, height: 8 }}
                role="status"
                aria-label={intl.formatMessage(i18n.loadingSpinner)}
              />
            </div>
          )}
        </span>
      }
      isStartExpanded={isStartExpanded}
    >
      <div
        ref={boxRef}
        className={`flex flex-col items-start space-y-2 overflow-y-auto p-4 ${working ? 'max-h-[4rem]' : 'max-h-[20rem]'}`}
      >
        {logs.map((log, i) => (
          <SubagentLogEntry key={i} log={log} />
        ))}
      </div>
    </ToolCallExpandable>
  );
}

const ProgressBar = ({ progress, total, message }: Omit<Progress, 'progressToken'>) => {
  const isDeterminate = typeof total === 'number';
  const percent = isDeterminate ? Math.min((progress / total) * 100, 100) : 0;

  return (
    <div className="w-full space-y-2">
      {message && <div className="font-sans text-sm text-textSubtle">{message}</div>}

      <div className="w-full bg-background-subtle rounded-full h-4 overflow-hidden relative">
        {isDeterminate ? (
          <div
            className="bg-primary h-full transition-all duration-300"
            style={{ width: `${percent}%` }}
          />
        ) : (
          <div className="absolute inset-0 animate-indeterminate bg-primary" />
        )}
      </div>
    </div>
  );
};
