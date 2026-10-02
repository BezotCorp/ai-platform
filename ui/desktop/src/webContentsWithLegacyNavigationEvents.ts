import type { WebContents, Event as ElectronEvent } from 'electron';

export type WebContentsWithLegacyNavigationEvents = WebContents & {
  on(
    event: 'new-window',
    listener: (event: ElectronEvent, url: string) => void
  ): WebContentsWithLegacyNavigationEvents;
  on(
    event: 'mouse-up',
    listener: (event: ElectronEvent, mouseButton: number) => void
  ): WebContentsWithLegacyNavigationEvents;
};
