import type { Client } from '@agentclientprotocol/sdk';
import type { GooseSessionNotificationUnstable } from '@aaif/goose-acp-client';
import type { ProviderDeviceCodeNotificationUnstable } from '@aaif/goose-acp-client';
import type { RecipeParamsResponseUnstable } from '@aaif/goose-acp-client';
import type { RequestRecipeParamsUnstable } from '@aaif/goose-acp-client';

export type GooseAcpCallbacks = Required<
  Pick<Client, 'requestPermission' | 'sessionUpdate' | 'createElicitation'>
> & {
  unstable_sessionRecipeRequestParams: (
    request: RequestRecipeParamsUnstable
  ) => Promise<RecipeParamsResponseUnstable>;
  unstable_sessionUpdate: (notification: GooseSessionNotificationUnstable) => Promise<void>;
  unstable_providerDeviceCode: (
    notification: ProviderDeviceCodeNotificationUnstable
  ) => Promise<void>;
};
