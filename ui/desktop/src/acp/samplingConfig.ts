import type { LocalInferenceModelSettingsDto } from '@aaif/goose-acp-client';

export type SamplingConfig = NonNullable<LocalInferenceModelSettingsDto['sampling']>;
