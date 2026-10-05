import type { AppInfo } from '@mcp-ui/client';
import type { RequestHandlerExtra } from '@mcp-ui/client';
import type { SandboxConfig } from '@mcp-ui/client';
import type { McpUiHostContext } from '@mcp-ui/client';
import type { McpUiSizeChangedNotification } from '@modelcontextprotocol/ext-apps/app-bridge';
import type { CallToolResult } from '@modelcontextprotocol/sdk/types.js';
import type { JSONRPCRequest } from '@modelcontextprotocol/sdk/types.js';

export interface GooseAppFrameProps {
  html: string;
  sandbox: SandboxConfig;
  hostContext: McpUiHostContext;
  toolInput?: Record<string, unknown>;
  toolInputPartial?: Record<string, unknown>;
  toolResult?: CallToolResult;
  toolCancelled?: boolean;
  onMessage: (params: {
    content: Array<{ type: string; text?: string }>;
  }) => Promise<Record<string, unknown>>;
  onOpenLink: (params: {
    url: string;
  }) => Promise<{ status: 'success' | 'error'; message?: string }>;
  onCallTool: (params: {
    name: string;
    arguments?: Record<string, unknown>;
  }) => Promise<CallToolResult>;
  onReadResource: (params: { uri: string }) => Promise<{
    contents: Array<{ uri: string; text: string; mimeType?: string }>;
  }>;
  onLoggingMessage: (params: { level?: string; logger?: string; data?: unknown }) => void;
  onFallbackRequest: (
    request: JSONRPCRequest,
    extra: RequestHandlerExtra
  ) => Promise<Record<string, unknown>>;
  onSizeChanged?: (params: McpUiSizeChangedNotification['params']) => void;
  onInitialized?: (appInfo: AppInfo) => void;
  onError?: (error: Error) => void;
}
