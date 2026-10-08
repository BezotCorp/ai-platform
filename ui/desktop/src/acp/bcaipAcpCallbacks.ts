import type { Client } from '@agentclientprotocol/sdk';
import type { BcaipSessionNotificationUnstable } from '@bezotcorp/bcaip-acp-client';
import type { ProviderDeviceCodeNotificationUnstable } from '@bezotcorp/bcaip-acp-client';
import type { RecipeParamsResponseUnstable } from '@bezotcorp/bcaip-acp-client';
import type { RequestRecipeParamsUnstable } from '@bezotcorp/bcaip-acp-client';

export type BcaipAcpCallbacks = Required<
  Pick<Client, 'requestPermission' | 'sessionUpdate' | 'createElicitation'>
> & {
  unstable_sessionRecipeRequestParams: (
    request: RequestRecipeParamsUnstable
  ) => Promise<RecipeParamsResponseUnstable>;
  unstable_sessionUpdate: (notification: BcaipSessionNotificationUnstable) => Promise<void>;
  unstable_providerDeviceCode: (
    notification: ProviderDeviceCodeNotificationUnstable
  ) => Promise<void>;
};
