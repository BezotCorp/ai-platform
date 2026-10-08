import {
  methods,
  type ForkSessionRequest,
  type ListSessionsRequest,
  type LoadSessionResponse,
  type NewSessionRequest,
  type SessionInfo,
} from '@agentclientprotocol/sdk';
import type { BcaipExtension, SessionExportFormatKey } from '@bezotcorp/bcaip-acp-client';
import { getAcpClient } from './acpConnection';
import { Session } from '../types/session';
import type { GooseSessionInfoMeta } from './bcaipSessionInfoMeta';
import { SessionListItem } from './sessionListItem';
import type { SessionListPage } from './sessionListPage';
import type { LoadSessionMeta } from './loadSessionMeta';
import type { AcpLoadSessionResult } from './acpLoadSessionResult';
import type { SessionListFilter } from './sessionListFilter';
import type { AcpNewSessionResult } from './acpNewSessionResult';
import type { AcpRecipeOptions } from './acpRecipeOptions';
import type { SessionType } from '../types/sessionType';

const inFlightSessionLoads = new Map<string, Promise<AcpLoadSessionResult>>();

function parseSessionResponseMeta(rawMeta: unknown): LoadSessionMeta {
  if (rawMeta === undefined || rawMeta === null) {
    return {};
  }

  if (!isRecord(rawMeta)) {
    throw new Error('Invalid load session metadata: expected object');
  }

  const workingDir = rawMeta.workingDir;

  if (workingDir !== undefined && typeof workingDir !== 'string') {
    throw new Error(
      "Invalid load session metadata 'workingDir': expected string"
    );
  }

  return {
    recipe: rawMeta.recipe as LoadSessionMeta['recipe'],
    userRecipeValues:
      rawMeta.userRecipeValues as LoadSessionMeta['userRecipeValues'],
    extensionResults:
      rawMeta.extensionResults as LoadSessionMeta['extensionResults'],
    workingDir,
  };
}

export function parseLoadMeta(response: LoadSessionResponse): LoadSessionMeta {
  return parseSessionResponseMeta(response._meta);
}

const SESSION_TYPES: readonly SessionType[] = [
  'user',
  'scheduled',
  'sub_agent',
  'hidden',
  'terminal',
  'gateway',
  'acp',
];

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function optionalString(
  record: Record<string, unknown>,
  key: string
): string | undefined {
  const value = record[key];

  if (value === undefined) {
    return undefined;
  }

  if (typeof value !== 'string') {
    throw new Error(`Invalid session metadata '${key}': expected string`);
  }

  return value;
}

function optionalBoolean(
  record: Record<string, unknown>,
  key: string
): boolean | undefined {
  const value = record[key];

  if (value === undefined) {
    return undefined;
  }

  if (typeof value !== 'boolean') {
    throw new Error(`Invalid session metadata '${key}': expected boolean`);
  }

  return value;
}

function optionalMessageCount(
  record: Record<string, unknown>
): number | undefined {
  const value = record.messageCount;

  if (value === undefined) {
    return undefined;
  }

  if (
    typeof value !== 'number' ||
    !Number.isInteger(value) ||
    value < 0
  ) {
    throw new Error(
      "Invalid session metadata 'messageCount': expected non-negative integer"
    );
  }

  return value;
}

function optionalSessionType(
  record: Record<string, unknown>
): SessionType | undefined {
  const value = record.sessionType;

  if (value === undefined) {
    return undefined;
  }

  if (
    typeof value !== 'string' ||
    !SESSION_TYPES.some((sessionType) => sessionType === value)
  ) {
    throw new Error(
      `Invalid session metadata 'sessionType': ${String(value)}`
    );
  }

  return value as SessionType;
}

function sessionInfoMeta(s: SessionInfo): GooseSessionInfoMeta {
  if (s._meta === undefined || s._meta === null) {
    return {};
  }

  if (!isRecord(s._meta)) {
    throw new Error('Invalid session metadata: expected object');
  }

  return {
    messageCount: optionalMessageCount(s._meta),
    createdAt: optionalString(s._meta, 'createdAt'),
    lastMessageAt: optionalString(s._meta, 'lastMessageAt'),
    archivedAt: optionalString(s._meta, 'archivedAt'),
    projectId: optionalString(s._meta, 'projectId'),
    providerId: optionalString(s._meta, 'providerId'),
    modelId: optionalString(s._meta, 'modelId'),
    sessionType: optionalSessionType(s._meta),
    userSetName: optionalBoolean(s._meta, 'userSetName'),
    hasRecipe: optionalBoolean(s._meta, 'hasRecipe'),
    lastMessageSnippet: optionalString(s._meta, 'lastMessageSnippet'),
  };
}

function normalizeSessionInfo(s: SessionInfo) {
  const meta = sessionInfoMeta(s);
  const createdAt = meta.createdAt ?? s.updatedAt ?? '';
  const updatedAt = s.updatedAt ?? createdAt;

  return {
    id: String(s.sessionId),
    name: s.title ?? '',
    workingDir: s.cwd,
    createdAt,
    updatedAt,
    lastMessageAt: meta.lastMessageAt,
    archivedAt: meta.archivedAt,
    messageCount: meta.messageCount ?? 0,
    projectId: meta.projectId,
    providerId: meta.providerId,
    modelId: meta.modelId,
    sessionType: meta.sessionType,
    userSetName: meta.userSetName,
    hasRecipe: meta.hasRecipe,
    lastMessageSnippet: meta.lastMessageSnippet,
  };
}

export function sessionInfoToSession(
  s: SessionInfo,
  loadMeta: LoadSessionMeta = {}
): Session {
  const normalized = normalizeSessionInfo(s);

  const modelConfig: Session['model_config'] = normalized.modelId
    ? {
        model_name: normalized.modelId,
        toolshim: false,
      }
    : null;

  return new Session({
    id: normalized.id,
    name: normalized.name,
    working_dir: loadMeta.workingDir ?? normalized.workingDir,
    created_at: normalized.createdAt,
    updated_at: normalized.updatedAt,
    last_message_at: normalized.lastMessageAt,
    message_count: normalized.messageCount,
    extension_data: {},
    archived_at: normalized.archivedAt,
    project_id: normalized.projectId,
    provider_name: normalized.providerId,
    model_config: modelConfig,
    session_type: normalized.sessionType,
    recipe: loadMeta.recipe,
    user_recipe_values: loadMeta.userRecipeValues,
    user_set_name: normalized.userSetName,
    last_message_snippet: normalized.lastMessageSnippet,
  });
}

function sessionInfoToListItem(s: SessionInfo): SessionListItem {
  const normalized = normalizeSessionInfo(s);

  return new SessionListItem({
    id: normalized.id,
    name: normalized.name,
    workingDir: normalized.workingDir,
    updatedAt: normalized.updatedAt,
    messageCount: normalized.messageCount,
    lastMessageAt: normalized.lastMessageAt,
    createdAt: normalized.createdAt,
    archivedAt: normalized.archivedAt,
    projectId: normalized.projectId,
    providerId: normalized.providerId,
    modelId: normalized.modelId,
    userSetName: normalized.userSetName,
    hasRecipe: normalized.hasRecipe,
    sessionType: normalized.sessionType,
  });
}

const SESSION_LIST_TYPES = ['user', 'scheduled'] as const;
const SESSION_LIST_TYPES_WITH_ACP = [...SESSION_LIST_TYPES, 'acp'] as const;

export async function acpListSessions(
  cursor?: string | null,
  filter: SessionListFilter = { includeAcp: false }
): Promise<SessionListPage> {
  const client = await getAcpClient();
  const request: ListSessionsRequest = {};
  if (cursor) {
    request.cursor = cursor;
  }
  const meta: Record<string, unknown> = {
    types: filter.includeAcp ? SESSION_LIST_TYPES_WITH_ACP : SESSION_LIST_TYPES,
  };
  const keyword = filter.keyword?.trim();
  if (keyword) {
    meta.query = keyword;
  }
  request._meta = meta;
  const response = await client.connection.agent.request(methods.agent.session.list, request);
  return {
    sessions: response.sessions.map(sessionInfoToListItem),
    nextCursor: response.nextCursor ?? null,
  };
}

export async function acpListRecentSessions(maxSessions: number): Promise<SessionListItem[]> {
  if (maxSessions <= 0) {
    return [];
  }

  const client = await getAcpClient();
  const response = await client.connection.agent.request(methods.agent.session.list, {
    _meta: { types: SESSION_LIST_TYPES },
  });
  return response.sessions.slice(0, maxSessions).map(sessionInfoToListItem);
}

export async function acpGetSessionListItem(sessionId: string): Promise<SessionListItem> {
  const client = await getAcpClient();
  const response = await client.bcaip.sessionInfoUnstable({ sessionId });
  return sessionInfoToListItem(response.session);
}

export async function acpLoadSession(sessionId: string): Promise<AcpLoadSessionResult> {
  const pendingLoad = inFlightSessionLoads.get(sessionId);
  if (pendingLoad) {
    return pendingLoad;
  }

  const loadPromise = loadAcpSession(sessionId);
  inFlightSessionLoads.set(sessionId, loadPromise);
  try {
    return await loadPromise;
  } finally {
    if (inFlightSessionLoads.get(sessionId) === loadPromise) {
      inFlightSessionLoads.delete(sessionId);
    }
  }
}

export function isAcpSessionLoadInFlight(sessionId: string): boolean {
  return inFlightSessionLoads.has(sessionId);
}

async function loadAcpSession(sessionId: string): Promise<AcpLoadSessionResult> {
  const client = await getAcpClient();
  const initialSessionInfoResponse = await client.bcaip.sessionInfoUnstable({ sessionId });
  const initialSessionInfo = initialSessionInfoResponse.session;
  const response = await client.connection.agent.request(methods.agent.session.load, {
    sessionId,
    cwd: initialSessionInfo.cwd,
    mcpServers: [],
  });
  // Loading can populate missing provider/model metadata.
  const sessionInfoResponse = await client.bcaip.sessionInfoUnstable({ sessionId });

  return {
    sessionInfo: sessionInfoResponse.session,
    response,
    meta: parseLoadMeta(response),
  };
}

/**
 * `bcaipExtensions` is three-valued: `undefined` leaves the key out so the backend
 * uses the configured set, while `[]` asks for a session with no extensions. The
 * backend already distinguishes the two, so the client has to as well.
 */
export async function acpNewSession(
  cwd: string,
  bcaipExtensions: BcaipExtension[] | undefined,
  recipe?: AcpRecipeOptions
): Promise<AcpNewSessionResult> {
  const client = await getAcpClient();
  const meta: Record<string, unknown> = { client: 'goose-desktop' };
  if (bcaipExtensions !== undefined) {
    meta.enabledExtensions = bcaipExtensions;
  }
  if (recipe?.recipeId) {
    meta.recipeId = recipe.recipeId;
  } else if (recipe?.recipeDeeplink) {
    meta.recipeDeeplink = recipe.recipeDeeplink;
  }
  if (recipe?.recipeParameterScopeId) {
    meta.recipeParameterScopeId = recipe.recipeParameterScopeId;
  }
  const request: NewSessionRequest = { cwd, mcpServers: [], _meta: meta };
  const response = await client.connection.agent.request(methods.agent.session.new, request);
  const sessionId = String(response.sessionId);
  const sessionInfoResponse = await client.bcaip.sessionInfoUnstable({ sessionId });

  return {
    sessionId,
    sessionInfo: sessionInfoResponse.session,
    meta: parseSessionResponseMeta(response._meta),
  };
}

export async function acpDeleteSession(sessionId: string): Promise<void> {
  const client = await getAcpClient();
  await client.connection.agent.request(methods.agent.session.delete, { sessionId });
}

export async function acpCloseSession(sessionId: string): Promise<void> {
  const client = await getAcpClient();
  await client.connection.agent.request(methods.agent.session.close, { sessionId });
}

export async function acpRenameSession(sessionId: string, title: string): Promise<void> {
  const client = await getAcpClient();
  await client.bcaip.sessionRenameUnstable({ sessionId, title });
}

export async function acpUpdateWorkingDir(sessionId: string, workingDir: string): Promise<void> {
  const client = await getAcpClient();
  await client.bcaip.sessionWorkingDirUpdateUnstable({ sessionId, workingDir });
}

export async function acpTruncateSessionConversation(
  sessionId: string,
  truncateFrom: number
): Promise<void> {
  const client = await getAcpClient();
  await client.bcaip.sessionConversationTruncateUnstable({ sessionId, truncateFrom });
}

export async function acpForkSession(
  sessionId: string,
  conversationBefore?: number
): Promise<string> {
  const client = await getAcpClient();
  const sessionInfo = await client.bcaip.sessionInfoUnstable({ sessionId });
  const { cwd } = sessionInfo.session;
  const request: ForkSessionRequest = { sessionId, cwd };
  if (conversationBefore !== undefined) {
    request._meta = { conversationBefore };
  }
  const response = await client.connection.agent.request(methods.agent.session.fork, request);
  return String(response.sessionId);
}

export async function acpExportSession(
  sessionId: string,
  format: SessionExportFormatKey = 'json'
): Promise<string> {
  const client = await getAcpClient();
  const response = await client.bcaip.sessionExportUnstable({ sessionId, format });
  return response.data;
}

export async function acpImportSession(input: string): Promise<void> {
  const client = await getAcpClient();
  await client.bcaip.sessionImportUnstable({ input });
}
