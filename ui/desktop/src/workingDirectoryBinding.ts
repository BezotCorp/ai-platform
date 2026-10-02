export type WorkingDirectoryBinding =
  | { status: 'ready'; path: string; dev: bigint; ino: bigint }
  | { status: 'missing'; path: string }
  | { status: 'error'; path: string };
