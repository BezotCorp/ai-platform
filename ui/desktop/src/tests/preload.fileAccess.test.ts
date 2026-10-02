import { beforeEach, describe, expect, it, vi } from 'vitest';

describe('preload file access boundary', () => {
  beforeEach(() => {
    vi.resetModules();
  });

  it('exposes only narrow file operations without renderer-supplied paths', async () => {
    const exposed: Record<string, unknown> = {};
    const invoke = vi.fn();
    vi.doMock('electron', () => ({
      default: {},
      contextBridge: {
        exposeInMainWorld: (name: string, api: unknown) => {
          exposed[name] = api;
        },
      },
      ipcRenderer: {
        emit: vi.fn(),
        invoke,
        off: vi.fn(),
        on: vi.fn(),
        removeListener: vi.fn(),
        send: vi.fn(),
        sendSync: vi.fn(),
      },
      webUtils: { getPathForFile: vi.fn() },
    }));

    await import('../preload');

    const electron = exposed.electron;

    if (typeof electron !== 'object' || electron === null) {
      throw new Error('Expected preload electron API');
    }

    expect(electron).not.toHaveProperty('readFile');

    const selectRecipeFile = Reflect.get(electron, 'selectRecipeFile');
    const readGoosehints = Reflect.get(electron, 'readGoosehints');
    const writeGoosehints = Reflect.get(electron, 'writeGoosehints');

    if (
      typeof selectRecipeFile !== 'function' ||
      typeof readGoosehints !== 'function' ||
      typeof writeGoosehints !== 'function'
    ) {
      throw new Error('Expected narrow preload file APIs');
    }

    selectRecipeFile('/etc/passwd');
    readGoosehints('../secret');
    writeGoosehints('project guidance', '../secret');

    expect(invoke).toHaveBeenNthCalledWith(1, 'select-recipe-file');
    expect(invoke).toHaveBeenNthCalledWith(2, 'read-goosehints');
    expect(invoke).toHaveBeenNthCalledWith(3, 'write-goosehints', 'project guidance');
  });
});
