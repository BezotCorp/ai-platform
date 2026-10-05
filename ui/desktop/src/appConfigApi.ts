export type AppConfigApi = {
  get: (key: string) => unknown;
  getAll: () => Record<string, unknown>;
};
