export type InferenceMetadata = {
  provider: string;
  requestedModel: string;
  resolvedModel?: string | null;
  providerSessionId?: string | null;
};
