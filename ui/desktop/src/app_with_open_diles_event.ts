import type { App, Event as ElectronEvent } from 'electron';

export type AppWithOpenFilesEvent = App & {
  on(
    event: 'open-files',
    listener: (event: ElectronEvent, filePaths: string[]) => void | Promise<void>
  ): AppWithOpenFilesEvent;
};
