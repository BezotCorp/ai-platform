export const LOCAL_MODEL = 'local-model' as const;
export const OWN_PROVIDER = 'own-provider' as const;

export type SelectedPath =
  | typeof LOCAL_MODEL
  | typeof OWN_PROVIDER
  | null;
