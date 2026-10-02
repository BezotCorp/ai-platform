export type ReadinessFetch = (
  input: string,
  init?: Parameters<typeof globalThis.fetch>[1]
) => Promise<Response>;
