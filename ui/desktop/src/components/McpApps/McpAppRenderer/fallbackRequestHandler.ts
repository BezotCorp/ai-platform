import type { RequestHandlerExtra } from '@mcp-ui/client';
import type { JSONRPCRequest } from '@modelcontextprotocol/sdk/types.js';

export type FallbackRequestHandler = {
  fallbackRequestHandler?: (
    request: JSONRPCRequest,
    extra: RequestHandlerExtra
  ) => Promise<Record<string, unknown>>;
};
