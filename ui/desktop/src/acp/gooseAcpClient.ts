import {
  client,
  methods,
  type Client,
  type ClientConnection,
  type Stream,
} from '@agentclientprotocol/sdk';
import {
  GOOSE_EXT_AGENT_REQUESTS,
  GOOSE_EXT_NOTIFICATIONS,
  GooseExtClient,
  type GooseSessionNotificationUnstable,
  type ProviderDeviceCodeNotificationUnstable,
  type RecipeParamsResponseUnstable,
  type RequestRecipeParamsUnstable,
  gooseSessionNotificationUnstableSchema,
  providerDeviceCodeNotificationUnstableSchema,
  requestRecipeParamsUnstableSchema,
} from '@aaif/goose-acp-client';

const [gooseSessionUpdate, providerDeviceCode] = GOOSE_EXT_NOTIFICATIONS;
const [gooseRecipeParamsRequest] = GOOSE_EXT_AGENT_REQUESTS;

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

export type GooseAcpClient = {
  connection: ClientConnection;
  goose: GooseExtClient;
};

export function connectGooseAcpClient(
  stream: Stream,
  callbacks: GooseAcpCallbacks
): GooseAcpClient {
  const app = client({ name: 'goose' })
    .onRequest(methods.client.session.requestPermission, (context) =>
      callbacks.requestPermission(context.params)
    )
    .onNotification(methods.client.session.update, (context) =>
      callbacks.sessionUpdate(context.params)
    )
    .onRequest(methods.client.elicitation.create, (context) =>
      callbacks.createElicitation(context.params)
    )
    .onRequest(gooseRecipeParamsRequest.method, requestRecipeParamsUnstableSchema, (context) =>
      callbacks.unstable_sessionRecipeRequestParams(context.params)
    )
    .onNotification(gooseSessionUpdate.method, gooseSessionNotificationUnstableSchema, (context) =>
      callbacks.unstable_sessionUpdate(context.params)
    )
    .onNotification(
      providerDeviceCode.method,
      providerDeviceCodeNotificationUnstableSchema,
      (context) => callbacks.unstable_providerDeviceCode(context.params)
    );

  const connection = app.connect(stream);
  const goose = new GooseExtClient(connection.agent);

  return { connection, goose };
}
