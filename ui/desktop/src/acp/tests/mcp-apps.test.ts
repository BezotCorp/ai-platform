import { beforeEach, describe, expect, it, vi } from 'vitest';
import { getAcpClient } from '../acpConnection';
import {
  deleteMcpApp,
  callMcpAppTool,
  exportMcpApp,
  importMcpApp,
  listMcpApps,
  listMcpAppTools,
  readMcpAppResource,
} from '../mcp-apps';

vi.mock('../acpConnection', () => ({
  getAcpClient: vi.fn(),
}));

function createClient() {
  return {
    bcaip: {
      resourcesReadUnstable: vi.fn(),
      toolsCallUnstable: vi.fn(),
      toolsListUnstable: vi.fn(),
      appsListUnstable: vi.fn(),
      appsExportUnstable: vi.fn(),
      appsImportUnstable: vi.fn(),
      appsDeleteUnstable: vi.fn(),
    },
  };
}

describe('ACP MCP app helpers', () => {
  let client: ReturnType<typeof createClient>;

  beforeEach(() => {
    vi.clearAllMocks();
    client = createClient();
    vi.mocked(getAcpClient).mockResolvedValue(
      client as unknown as Awaited<ReturnType<typeof getAcpClient>>
    );
  });

  it('flattens ACP resource reads into the renderer resource shape', async () => {
    client.bcaip.resourcesReadUnstable.mockResolvedValue({
      result: {
        contents: [
          {
            uri: 'ui://weather/panel',
            mimeType: 'text/html;profile=mcp-app',
            text: '<main>Weather</main>',
            _meta: {
              ui: {
                csp: {
                  connectDomains: ['https://api.example.com'],
                },
                prefersBorder: false,
              },
            },
          },
        ],
      },
    });

    const resource = await readMcpAppResource('session-1', 'weather', 'ui://weather/panel');

    expect(client.bcaip.resourcesReadUnstable).toHaveBeenCalledWith({
      sessionId: 'session-1',
      extensionName: 'weather',
      uri: 'ui://weather/panel',
    });
    expect(resource).toEqual({
      uri: 'ui://weather/panel',
      mimeType: 'text/html;profile=mcp-app',
      text: '<main>Weather</main>',
      _meta: {
        ui: {
          csp: {
            connectDomains: ['https://api.example.com'],
          },
          prefersBorder: false,
        },
      },
    });
  });

  it('decodes blob resources as UTF-8 text', async () => {
    client.bcaip.resourcesReadUnstable.mockResolvedValue({
      result: {
        contents: [
          {
            uri: 'ui://weather/panel',
            mimeType: 'text/html;profile=mcp-app',
            blob: Buffer.from('<main>São Paulo 東京</main>', 'utf8').toString('base64'),
          },
        ],
      },
    });

    const resource = await readMcpAppResource('session-1', 'weather', 'ui://weather/panel');

    expect(resource.text).toBe('<main>São Paulo 東京</main>');
  });

  it('prefixes app tool calls before sending them over ACP', async () => {
    client.bcaip.toolsCallUnstable.mockResolvedValue({
      content: [{ type: 'text', text: 'done' }],
      structuredContent: { ok: true },
      isError: false,
      _meta: { traceId: 'trace-1' },
    });

    const result = await callMcpAppTool('session-1', 'weather', 'refresh', { city: 'Amsterdam' });

    expect(client.bcaip.toolsCallUnstable).toHaveBeenCalledWith({
      sessionId: 'session-1',
      extensionName: 'weather',
      name: 'weather__refresh',
      arguments: { city: 'Amsterdam' },
    });
    expect(result).toEqual({
      content: [{ type: 'text', text: 'done' }],
      structuredContent: { ok: true },
      isError: false,
      _meta: { traceId: 'trace-1' },
    });
  });

  it('maps and filters ACP tools for app host context', async () => {
    client.bcaip.toolsListUnstable.mockResolvedValue({
      tools: [
        {
          name: 'weather__refresh',
          description: 'Refresh weather',
          parameters: [],
          inputSchema: {
            type: 'object',
            properties: {
              city: { type: 'string' },
            },
          },
        },
        {
          name: 'calendar__refresh',
          description: 'Refresh calendar',
          parameters: [],
          inputSchema: { type: 'object' },
        },
      ],
    });

    const tools = await listMcpAppTools('session-1', 'weather');

    expect(client.bcaip.toolsListUnstable).toHaveBeenCalledWith({ sessionId: 'session-1' });
    expect(tools).toEqual([
      {
        name: 'weather__refresh',
        description: 'Refresh weather',
        parameters: [],
        inputSchema: {
          type: 'object',
          properties: {
            city: { type: 'string' },
          },
        },
      },
    ]);
  });

  it('lists apps through ACP', async () => {
    client.bcaip.appsListUnstable.mockResolvedValue({
      apps: [
        {
          uri: 'ui://apps/weather',
          name: 'weather',
          mimeType: 'text/html;profile=mcp-app',
          text: '<main>Weather</main>',
          mcpServers: ['apps'],
        },
      ],
    });

    const apps = await listMcpApps('session-1');

    expect(client.bcaip.appsListUnstable).toHaveBeenCalledWith({ sessionId: 'session-1' });
    expect(apps).toEqual([
      {
        uri: 'ui://apps/weather',
        name: 'weather',
        mimeType: 'text/html;profile=mcp-app',
        text: '<main>Weather</main>',
        mcpServers: ['apps'],
      },
    ]);
  });

  it('imports and exports apps through ACP', async () => {
    client.bcaip.appsExportUnstable.mockResolvedValue({
      html: '<html><body>Weather</body></html>',
    });
    client.bcaip.appsImportUnstable.mockResolvedValue({
      name: 'weather',
      message: 'ok',
    });

    await expect(exportMcpApp('weather')).resolves.toBe('<html><body>Weather</body></html>');
    await importMcpApp('<html><body>Weather</body></html>');

    expect(client.bcaip.appsExportUnstable).toHaveBeenCalledWith({ name: 'weather' });
    expect(client.bcaip.appsImportUnstable).toHaveBeenCalledWith({
      html: '<html><body>Weather</body></html>',
    });
  });

  it('deletes apps through ACP', async () => {
    client.bcaip.appsDeleteUnstable.mockResolvedValue({
      name: 'weather',
      message: 'App deleted',
    });

    await deleteMcpApp('weather');

    expect(client.bcaip.appsDeleteUnstable).toHaveBeenCalledWith({ name: 'weather' });
  });

  it('normalizes ACP delete errors', async () => {
    client.bcaip.appsDeleteUnstable.mockRejectedValue({
      error: { data: 'Cannot delete default app' },
    });

    await expect(deleteMcpApp('clock')).rejects.toThrow('Cannot delete default app');
  });

  it('normalizes ACP export errors', async () => {
    client.bcaip.appsExportUnstable.mockRejectedValue({
      error: { message: 'App not found' },
    });

    await expect(exportMcpApp('missing')).rejects.toThrow('App not found');
  });
});
