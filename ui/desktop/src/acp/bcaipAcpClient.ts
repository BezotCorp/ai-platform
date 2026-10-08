import { client, methods, type ClientConnection, type Stream } from '@agentclientprotocol/sdk';
import {
  BCAIP_EXT_AGENT_REQUESTS,
  BCAIP_EXT_NOTIFICATIONS,
  BcaipExtClient,
  bcaipSessionNotificationUnstableSchema,
  providerDeviceCodeNotificationUnstableSchema,
  requestRecipeParamsUnstableSchema,
} from '@bezotcorp/bcaip-acp-client';

import type { BcaipAcpCallbacks } from './bcaipAcpCallbacks';
const [bcaipSessionUpdate, providerDeviceCode] = BCAIP_EXT_NOTIFICATIONS;
const [bcaipRecipeParamsRequest] = BCAIP_EXT_AGENT_REQUESTS;

export type BcaipAcpClient = {
  connection: ClientConnection;
  bcaip: BcaipExtClient;
};

export function connectBcaipAcpClient(
  stream: Stream,
  callbacks: BcaipAcpCallbacks
): BcaipAcpClient {
  const app = client({ name: 'bcaip' })
    .onRequest(methods.client.session.requestPermission, (context) =>
      callbacks.requestPermission(context.params)
    )
    .onNotification(methods.client.session.update, (context) =>
      callbacks.sessionUpdate(context.params)
    )
    .onRequest(methods.client.elicitation.create, (context) =>
      callbacks.createElicitation(context.params)
    )
    .onRequest(bcaipRecipeParamsRequest.method, requestRecipeParamsUnstableSchema, (context) =>
      callbacks.unstable_sessionRecipeRequestParams(context.params)
    )
    .onNotification(bcaipSessionUpdate.method, bcaipSessionNotificationUnstableSchema, (context) =>
      callbacks.unstable_sessionUpdate(context.params)
    )
    .onNotification(
      providerDeviceCode.method,
      providerDeviceCodeNotificationUnstableSchema,
      (context) => callbacks.unstable_providerDeviceCode(context.params)
    );

  const connection = app.connect(stream);
  const bcaip = new BcaipExtClient(connection.agent);

  return { connection, bcaip };
}
