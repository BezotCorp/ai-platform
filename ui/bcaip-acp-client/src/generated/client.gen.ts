// This file is auto-generated — do not edit manually.

import type { ClientContext } from "@agentclientprotocol/sdk";
import type {
  AddConfigExtensionRequestUnstable,
  AddSessionExtensionRequestUnstable,
  AppsDeleteRequestUnstable,
  AppsDeleteResponseUnstable,
  AppsExportRequestUnstable,
  AppsExportResponseUnstable,
  AppsImportRequestUnstable,
  AppsImportResponseUnstable,
  AppsListRequestUnstable,
  AppsListResponseUnstable,
  ArchiveSessionRequestUnstable,
  BcaipToolCallRequestUnstable,
  BcaipToolCallResponseUnstable,
  CanonicalModelInfoRequestUnstable,
  CanonicalModelInfoResponseUnstable,
  ConfigReadAllRequestUnstable,
  ConfigReadAllResponseUnstable,
  ConfigReadRequestUnstable,
  ConfigReadResponseUnstable,
  ConfigRemoveRequestUnstable,
  ConfigUpsertRequestUnstable,
  CreateScheduleRequestUnstable,
  CreateScheduleResponseUnstable,
  CreateSourceRequestUnstable,
  CreateSourceResponseUnstable,
  CustomProviderCreateRequestUnstable,
  CustomProviderCreateResponseUnstable,
  CustomProviderDeleteRequestUnstable,
  CustomProviderDeleteResponseUnstable,
  CustomProviderReadRequestUnstable,
  CustomProviderReadResponseUnstable,
  CustomProviderUpdateRequestUnstable,
  CustomProviderUpdateResponseUnstable,
  DecodeRecipeRequestUnstable,
  DecodeRecipeResponseUnstable,
  DefaultsClearRequestUnstable,
  DefaultsReadRequestUnstable,
  DefaultsReadResponseUnstable,
  DefaultsSaveRequestUnstable,
  DeleteRecipeRequestUnstable,
  DeleteScheduleRequestUnstable,
  DeleteSourceRequestUnstable,
  DiagnosticsGetRequestUnstable,
  DiagnosticsGetResponseUnstable,
  DictationConfigRequestUnstable,
  DictationConfigResponseUnstable,
  DictationModelCancelRequestUnstable,
  DictationModelDeleteRequestUnstable,
  DictationModelDownloadProgressRequestUnstable,
  DictationModelDownloadProgressResponseUnstable,
  DictationModelDownloadRequestUnstable,
  DictationModelsListRequestUnstable,
  DictationModelsListResponseUnstable,
  DictationTranscribeRequestUnstable,
  DictationTranscribeResponseUnstable,
  EncodeRecipeRequestUnstable,
  EncodeRecipeResponseUnstable,
  ExportSessionRequestUnstable,
  ExportSessionResponseUnstable,
  ExportSourceRequestUnstable,
  ExportSourceResponseUnstable,
  GetConfigExtensionsRequestUnstable,
  GetConfigExtensionsResponseUnstable,
  GetPromptRequestUnstable,
  GetPromptResponseUnstable,
  GetSessionExtensionsRequestUnstable,
  GetSessionExtensionsResponseUnstable,
  GetSessionInfoRequestUnstable,
  GetSessionInfoResponseUnstable,
  GetToolsRequestUnstable,
  GetToolsResponseUnstable,
  ImportSessionRequestUnstable,
  ImportSessionResponseUnstable,
  ImportSourcesRequestUnstable,
  ImportSourcesResponseUnstable,
  InspectRunningJobRequestUnstable,
  InspectRunningJobResponseUnstable,
  KillRunningJobRequestUnstable,
  KillRunningJobResponseUnstable,
  ListAgentMentionsRequestUnstable,
  ListAgentMentionsResponseUnstable,
  ListPromptsRequestUnstable,
  ListPromptsResponseUnstable,
  ListProvidersRequestUnstable,
  ListProvidersResponseUnstable,
  ListRecipesRequestUnstable,
  ListRecipesResponseUnstable,
  ListScheduleSessionsRequestUnstable,
  ListScheduleSessionsResponseUnstable,
  ListSchedulesRequestUnstable,
  ListSchedulesResponseUnstable,
  ListSlashCommandsRequestUnstable,
  ListSlashCommandsResponseUnstable,
  ListSourcesRequestUnstable,
  ListSourcesResponseUnstable,
  LiveVoiceAvailabilityRequestUnstable,
  LiveVoiceAvailabilityResponseUnstable,
  LiveVoiceStartRequestUnstable,
  LiveVoiceStartResponseUnstable,
  LiveVoiceStopRequestUnstable,
  LocalInferenceBuiltinChatTemplatesListRequestUnstable,
  LocalInferenceBuiltinChatTemplatesListResponseUnstable,
  LocalInferenceHuggingFaceRepoVariantsRequestUnstable,
  LocalInferenceHuggingFaceRepoVariantsResponseUnstable,
  LocalInferenceHuggingFaceSearchRequestUnstable,
  LocalInferenceHuggingFaceSearchResponseUnstable,
  LocalInferenceModelDeleteRequestUnstable,
  LocalInferenceModelDownloadCancelRequestUnstable,
  LocalInferenceModelDownloadProgressRequestUnstable,
  LocalInferenceModelDownloadProgressResponseUnstable,
  LocalInferenceModelDownloadRequestUnstable,
  LocalInferenceModelDownloadResponseUnstable,
  LocalInferenceModelEvictRequestUnstable,
  LocalInferenceModelSettingsReadRequestUnstable,
  LocalInferenceModelSettingsReadResponseUnstable,
  LocalInferenceModelSettingsUpdateRequestUnstable,
  LocalInferenceModelSettingsUpdateResponseUnstable,
  LocalInferenceModelsListRequestUnstable,
  LocalInferenceModelsListResponseUnstable,
  OnboardingImportApplyRequestUnstable,
  OnboardingImportApplyResponseUnstable,
  OnboardingImportScanRequestUnstable,
  OnboardingImportScanResponseUnstable,
  ParseRecipeRequestUnstable,
  ParseRecipeResponseUnstable,
  PauseScheduleRequestUnstable,
  PreferencesReadRequestUnstable,
  PreferencesReadResponseUnstable,
  PreferencesSaveRequestUnstable,
  PromptOperationResponseUnstable,
  ProviderCatalogListRequestUnstable,
  ProviderCatalogListResponseUnstable,
  ProviderCatalogTemplateRequestUnstable,
  ProviderCatalogTemplateResponseUnstable,
  ProviderConfigAuthenticateRequestUnstable,
  ProviderConfigChangeResponseUnstable,
  ProviderConfigDeleteRequestUnstable,
  ProviderConfigReadRequestUnstable,
  ProviderConfigReadResponseUnstable,
  ProviderConfigSaveRequestUnstable,
  ProviderConfigStatusRequestUnstable,
  ProviderConfigStatusResponseUnstable,
  ProviderReadinessCheckRequestUnstable,
  ProviderReadinessCheckResponseUnstable,
  ProviderSecretDeleteRequestUnstable,
  ProviderSecretsListRequestUnstable,
  ProviderSecretsListResponseUnstable,
  ProviderSetupCatalogListRequestUnstable,
  ProviderSetupCatalogListResponseUnstable,
  ProviderSupportedModelsListRequestUnstable,
  ProviderSupportedModelsListResponseUnstable,
  ReadResourceRequestUnstable,
  ReadResourceResponseUnstable,
  RecipeToYamlRequestUnstable,
  RecipeToYamlResponseUnstable,
  RefreshProviderInventoryRequestUnstable,
  RefreshProviderInventoryResponseUnstable,
  RemoveConfigExtensionRequestUnstable,
  RemoveSessionExtensionRequestUnstable,
  RenameSessionRequestUnstable,
  ResetPromptRequestUnstable,
  RunScheduleNowRequestUnstable,
  RunScheduleNowResponseUnstable,
  SavePromptRequestUnstable,
  SaveRecipeRequestUnstable,
  SaveRecipeResponseUnstable,
  ScanRecipeRequestUnstable,
  ScanRecipeResponseUnstable,
  ScheduleRecipeRequestUnstable,
  SetConfigExtensionEnabledRequestUnstable,
  SetRecipeSlashCommandRequestUnstable,
  SetSessionSystemPromptRequestUnstable,
  SetToolPermissionsRequestUnstable,
  SetToolPermissionsResponseUnstable,
  SteerSessionRequestUnstable,
  SteerSessionResponseUnstable,
  TruncateSessionConversationRequestUnstable,
  UnarchiveSessionRequestUnstable,
  UnpauseScheduleRequestUnstable,
  UpdateScheduleRequestUnstable,
  UpdateScheduleResponseUnstable,
  UpdateSessionProjectRequestUnstable,
  UpdateSourceRequestUnstable,
  UpdateSourceResponseUnstable,
  UpdateWorkingDirRequestUnstable,
} from "./types.gen.js";
import {
  appsDeleteResponseUnstableSchema,
  appsExportResponseUnstableSchema,
  appsImportResponseUnstableSchema,
  appsListResponseUnstableSchema,
  bcaipToolCallResponseUnstableSchema,
  canonicalModelInfoResponseUnstableSchema,
  configReadAllResponseUnstableSchema,
  configReadResponseUnstableSchema,
  createScheduleResponseUnstableSchema,
  createSourceResponseUnstableSchema,
  customProviderCreateResponseUnstableSchema,
  customProviderDeleteResponseUnstableSchema,
  customProviderReadResponseUnstableSchema,
  customProviderUpdateResponseUnstableSchema,
  decodeRecipeResponseUnstableSchema,
  defaultsReadResponseUnstableSchema,
  diagnosticsGetResponseUnstableSchema,
  dictationConfigResponseUnstableSchema,
  dictationModelDownloadProgressResponseUnstableSchema,
  dictationModelsListResponseUnstableSchema,
  dictationTranscribeResponseUnstableSchema,
  encodeRecipeResponseUnstableSchema,
  exportSessionResponseUnstableSchema,
  exportSourceResponseUnstableSchema,
  getConfigExtensionsResponseUnstableSchema,
  getPromptResponseUnstableSchema,
  getSessionExtensionsResponseUnstableSchema,
  getSessionInfoResponseUnstableSchema,
  getToolsResponseUnstableSchema,
  importSessionResponseUnstableSchema,
  importSourcesResponseUnstableSchema,
  inspectRunningJobResponseUnstableSchema,
  killRunningJobResponseUnstableSchema,
  listAgentMentionsResponseUnstableSchema,
  listPromptsResponseUnstableSchema,
  listProvidersResponseUnstableSchema,
  listRecipesResponseUnstableSchema,
  listScheduleSessionsResponseUnstableSchema,
  listSchedulesResponseUnstableSchema,
  listSlashCommandsResponseUnstableSchema,
  listSourcesResponseUnstableSchema,
  liveVoiceAvailabilityResponseUnstableSchema,
  liveVoiceStartResponseUnstableSchema,
  localInferenceBuiltinChatTemplatesListResponseUnstableSchema,
  localInferenceHuggingFaceRepoVariantsResponseUnstableSchema,
  localInferenceHuggingFaceSearchResponseUnstableSchema,
  localInferenceModelDownloadProgressResponseUnstableSchema,
  localInferenceModelDownloadResponseUnstableSchema,
  localInferenceModelSettingsReadResponseUnstableSchema,
  localInferenceModelSettingsUpdateResponseUnstableSchema,
  localInferenceModelsListResponseUnstableSchema,
  onboardingImportApplyResponseUnstableSchema,
  onboardingImportScanResponseUnstableSchema,
  parseRecipeResponseUnstableSchema,
  preferencesReadResponseUnstableSchema,
  promptOperationResponseUnstableSchema,
  providerCatalogListResponseUnstableSchema,
  providerCatalogTemplateResponseUnstableSchema,
  providerConfigChangeResponseUnstableSchema,
  providerConfigReadResponseUnstableSchema,
  providerConfigStatusResponseUnstableSchema,
  providerReadinessCheckResponseUnstableSchema,
  providerSecretsListResponseUnstableSchema,
  providerSetupCatalogListResponseUnstableSchema,
  providerSupportedModelsListResponseUnstableSchema,
  readResourceResponseUnstableSchema,
  recipeToYamlResponseUnstableSchema,
  refreshProviderInventoryResponseUnstableSchema,
  runScheduleNowResponseUnstableSchema,
  saveRecipeResponseUnstableSchema,
  scanRecipeResponseUnstableSchema,
  setToolPermissionsResponseUnstableSchema,
  steerSessionResponseUnstableSchema,
  updateScheduleResponseUnstableSchema,
  updateSourceResponseUnstableSchema,
} from "./zod.gen.js";

export class BcaipExtClient {
  constructor(private conn: Pick<ClientContext, "request">) {}

  async sessionExtensionsAddUnstable(
    params: AddSessionExtensionRequestUnstable,
  ): Promise<void> {
    await this.conn.request("_bcaip/unstable/session/extensions/add", params);
  }

  async sessionExtensionsRemoveUnstable(
    params: RemoveSessionExtensionRequestUnstable,
  ): Promise<void> {
    await this.conn.request(
      "_bcaip/unstable/session/extensions/remove",
      params,
    );
  }

  async toolsListUnstable(
    params: GetToolsRequestUnstable,
  ): Promise<GetToolsResponseUnstable> {
    const raw = await this.conn.request("_bcaip/unstable/tools/list", params);
    return getToolsResponseUnstableSchema.parse(
      raw,
    ) as GetToolsResponseUnstable;
  }

  async toolsPermissionsSetUnstable(
    params: SetToolPermissionsRequestUnstable,
  ): Promise<SetToolPermissionsResponseUnstable> {
    const raw = await this.conn.request(
      "_bcaip/unstable/tools/permissions/set",
      params,
    );
    return setToolPermissionsResponseUnstableSchema.parse(
      raw,
    ) as SetToolPermissionsResponseUnstable;
  }

  async toolsCallUnstable(
    params: BcaipToolCallRequestUnstable,
  ): Promise<BcaipToolCallResponseUnstable> {
    const raw = await this.conn.request("_bcaip/unstable/tools/call", params);
    return bcaipToolCallResponseUnstableSchema.parse(
      raw,
    ) as BcaipToolCallResponseUnstable;
  }

  async resourcesReadUnstable(
    params: ReadResourceRequestUnstable,
  ): Promise<ReadResourceResponseUnstable> {
    const raw = await this.conn.request(
      "_bcaip/unstable/resources/read",
      params,
    );
    return readResourceResponseUnstableSchema.parse(
      raw,
    ) as ReadResourceResponseUnstable;
  }

  async appsListUnstable(
    params: AppsListRequestUnstable,
  ): Promise<AppsListResponseUnstable> {
    const raw = await this.conn.request("_bcaip/unstable/apps/list", params);
    return appsListResponseUnstableSchema.parse(
      raw,
    ) as AppsListResponseUnstable;
  }

  async appsExportUnstable(
    params: AppsExportRequestUnstable,
  ): Promise<AppsExportResponseUnstable> {
    const raw = await this.conn.request("_bcaip/unstable/apps/export", params);
    return appsExportResponseUnstableSchema.parse(
      raw,
    ) as AppsExportResponseUnstable;
  }

  async appsImportUnstable(
    params: AppsImportRequestUnstable,
  ): Promise<AppsImportResponseUnstable> {
    const raw = await this.conn.request("_bcaip/unstable/apps/import", params);
    return appsImportResponseUnstableSchema.parse(
      raw,
    ) as AppsImportResponseUnstable;
  }

  async appsDeleteUnstable(
    params: AppsDeleteRequestUnstable,
  ): Promise<AppsDeleteResponseUnstable> {
    const raw = await this.conn.request("_bcaip/unstable/apps/delete", params);
    return appsDeleteResponseUnstableSchema.parse(
      raw,
    ) as AppsDeleteResponseUnstable;
  }

  async sessionWorkingDirUpdateUnstable(
    params: UpdateWorkingDirRequestUnstable,
  ): Promise<void> {
    await this.conn.request(
      "_bcaip/unstable/session/working-dir/update",
      params,
    );
  }

  async sessionSystemPromptSetUnstable(
    params: SetSessionSystemPromptRequestUnstable,
  ): Promise<void> {
    await this.conn.request(
      "_bcaip/unstable/session/system-prompt/set",
      params,
    );
  }

  async sessionSteerUnstable(
    params: SteerSessionRequestUnstable,
  ): Promise<SteerSessionResponseUnstable> {
    const raw = await this.conn.request(
      "_bcaip/unstable/session/steer",
      params,
    );
    return steerSessionResponseUnstableSchema.parse(
      raw,
    ) as SteerSessionResponseUnstable;
  }

  async sessionLiveVoiceAvailabilityUnstable(
    params: LiveVoiceAvailabilityRequestUnstable,
  ): Promise<LiveVoiceAvailabilityResponseUnstable> {
    const raw = await this.conn.request(
      "_bcaip/unstable/session/live-voice/availability",
      params,
    );
    return liveVoiceAvailabilityResponseUnstableSchema.parse(
      raw,
    ) as LiveVoiceAvailabilityResponseUnstable;
  }

  async sessionLiveVoiceStartUnstable(
    params: LiveVoiceStartRequestUnstable,
  ): Promise<LiveVoiceStartResponseUnstable> {
    const raw = await this.conn.request(
      "_bcaip/unstable/session/live-voice/start",
      params,
    );
    return liveVoiceStartResponseUnstableSchema.parse(
      raw,
    ) as LiveVoiceStartResponseUnstable;
  }

  async sessionLiveVoiceStopUnstable(
    params: LiveVoiceStopRequestUnstable,
  ): Promise<void> {
    await this.conn.request("_bcaip/unstable/session/live-voice/stop", params);
  }

  async diagnosticsGetUnstable(
    params: DiagnosticsGetRequestUnstable,
  ): Promise<DiagnosticsGetResponseUnstable> {
    const raw = await this.conn.request(
      "_bcaip/unstable/diagnostics/get",
      params,
    );
    return diagnosticsGetResponseUnstableSchema.parse(
      raw,
    ) as DiagnosticsGetResponseUnstable;
  }

  async configPromptsListUnstable(
    params: ListPromptsRequestUnstable,
  ): Promise<ListPromptsResponseUnstable> {
    const raw = await this.conn.request(
      "_bcaip/unstable/config/prompts/list",
      params,
    );
    return listPromptsResponseUnstableSchema.parse(
      raw,
    ) as ListPromptsResponseUnstable;
  }

  async configPromptsGetUnstable(
    params: GetPromptRequestUnstable,
  ): Promise<GetPromptResponseUnstable> {
    const raw = await this.conn.request(
      "_bcaip/unstable/config/prompts/get",
      params,
    );
    return getPromptResponseUnstableSchema.parse(
      raw,
    ) as GetPromptResponseUnstable;
  }

  async configPromptsSaveUnstable(
    params: SavePromptRequestUnstable,
  ): Promise<PromptOperationResponseUnstable> {
    const raw = await this.conn.request(
      "_bcaip/unstable/config/prompts/save",
      params,
    );
    return promptOperationResponseUnstableSchema.parse(
      raw,
    ) as PromptOperationResponseUnstable;
  }

  async configPromptsResetUnstable(
    params: ResetPromptRequestUnstable,
  ): Promise<PromptOperationResponseUnstable> {
    const raw = await this.conn.request(
      "_bcaip/unstable/config/prompts/reset",
      params,
    );
    return promptOperationResponseUnstableSchema.parse(
      raw,
    ) as PromptOperationResponseUnstable;
  }

  async configExtensionsListUnstable(
    params: GetConfigExtensionsRequestUnstable,
  ): Promise<GetConfigExtensionsResponseUnstable> {
    const raw = await this.conn.request(
      "_bcaip/unstable/config/extensions/list",
      params,
    );
    return getConfigExtensionsResponseUnstableSchema.parse(
      raw,
    ) as GetConfigExtensionsResponseUnstable;
  }

  async configExtensionsAddUnstable(
    params: AddConfigExtensionRequestUnstable,
  ): Promise<void> {
    await this.conn.request("_bcaip/unstable/config/extensions/add", params);
  }

  async configExtensionsRemoveUnstable(
    params: RemoveConfigExtensionRequestUnstable,
  ): Promise<void> {
    await this.conn.request("_bcaip/unstable/config/extensions/remove", params);
  }

  async configExtensionsSetEnabledUnstable(
    params: SetConfigExtensionEnabledRequestUnstable,
  ): Promise<void> {
    await this.conn.request(
      "_bcaip/unstable/config/extensions/set-enabled",
      params,
    );
  }

  async sessionExtensionsListUnstable(
    params: GetSessionExtensionsRequestUnstable,
  ): Promise<GetSessionExtensionsResponseUnstable> {
    const raw = await this.conn.request(
      "_bcaip/unstable/session/extensions/list",
      params,
    );
    return getSessionExtensionsResponseUnstableSchema.parse(
      raw,
    ) as GetSessionExtensionsResponseUnstable;
  }

  async providersListUnstable(
    params: ListProvidersRequestUnstable,
  ): Promise<ListProvidersResponseUnstable> {
    const raw = await this.conn.request(
      "_bcaip/unstable/providers/list",
      params,
    );
    return listProvidersResponseUnstableSchema.parse(
      raw,
    ) as ListProvidersResponseUnstable;
  }

  async providersSupportedModelsListUnstable(
    params: ProviderSupportedModelsListRequestUnstable,
  ): Promise<ProviderSupportedModelsListResponseUnstable> {
    const raw = await this.conn.request(
      "_bcaip/unstable/providers/supported-models/list",
      params,
    );
    return providerSupportedModelsListResponseUnstableSchema.parse(
      raw,
    ) as ProviderSupportedModelsListResponseUnstable;
  }

  async providersCatalogListUnstable(
    params: ProviderCatalogListRequestUnstable,
  ): Promise<ProviderCatalogListResponseUnstable> {
    const raw = await this.conn.request(
      "_bcaip/unstable/providers/catalog/list",
      params,
    );
    return providerCatalogListResponseUnstableSchema.parse(
      raw,
    ) as ProviderCatalogListResponseUnstable;
  }

  async providersSetupCatalogListUnstable(
    params: ProviderSetupCatalogListRequestUnstable,
  ): Promise<ProviderSetupCatalogListResponseUnstable> {
    const raw = await this.conn.request(
      "_bcaip/unstable/providers/setup/catalog/list",
      params,
    );
    return providerSetupCatalogListResponseUnstableSchema.parse(
      raw,
    ) as ProviderSetupCatalogListResponseUnstable;
  }

  async providersCatalogTemplateUnstable(
    params: ProviderCatalogTemplateRequestUnstable,
  ): Promise<ProviderCatalogTemplateResponseUnstable> {
    const raw = await this.conn.request(
      "_bcaip/unstable/providers/catalog/template",
      params,
    );
    return providerCatalogTemplateResponseUnstableSchema.parse(
      raw,
    ) as ProviderCatalogTemplateResponseUnstable;
  }

  async providersCustomCreateUnstable(
    params: CustomProviderCreateRequestUnstable,
  ): Promise<CustomProviderCreateResponseUnstable> {
    const raw = await this.conn.request(
      "_bcaip/unstable/providers/custom/create",
      params,
    );
    return customProviderCreateResponseUnstableSchema.parse(
      raw,
    ) as CustomProviderCreateResponseUnstable;
  }

  async providersCustomReadUnstable(
    params: CustomProviderReadRequestUnstable,
  ): Promise<CustomProviderReadResponseUnstable> {
    const raw = await this.conn.request(
      "_bcaip/unstable/providers/custom/read",
      params,
    );
    return customProviderReadResponseUnstableSchema.parse(
      raw,
    ) as CustomProviderReadResponseUnstable;
  }

  async providersCustomUpdateUnstable(
    params: CustomProviderUpdateRequestUnstable,
  ): Promise<CustomProviderUpdateResponseUnstable> {
    const raw = await this.conn.request(
      "_bcaip/unstable/providers/custom/update",
      params,
    );
    return customProviderUpdateResponseUnstableSchema.parse(
      raw,
    ) as CustomProviderUpdateResponseUnstable;
  }

  async providersCustomDeleteUnstable(
    params: CustomProviderDeleteRequestUnstable,
  ): Promise<CustomProviderDeleteResponseUnstable> {
    const raw = await this.conn.request(
      "_bcaip/unstable/providers/custom/delete",
      params,
    );
    return customProviderDeleteResponseUnstableSchema.parse(
      raw,
    ) as CustomProviderDeleteResponseUnstable;
  }

  async providersInventoryRefreshUnstable(
    params: RefreshProviderInventoryRequestUnstable,
  ): Promise<RefreshProviderInventoryResponseUnstable> {
    const raw = await this.conn.request(
      "_bcaip/unstable/providers/inventory/refresh",
      params,
    );
    return refreshProviderInventoryResponseUnstableSchema.parse(
      raw,
    ) as RefreshProviderInventoryResponseUnstable;
  }

  async providersReadinessCheckUnstable(
    params: ProviderReadinessCheckRequestUnstable,
  ): Promise<ProviderReadinessCheckResponseUnstable> {
    const raw = await this.conn.request(
      "_bcaip/unstable/providers/readiness/check",
      params,
    );
    return providerReadinessCheckResponseUnstableSchema.parse(
      raw,
    ) as ProviderReadinessCheckResponseUnstable;
  }

  async providersConfigReadUnstable(
    params: ProviderConfigReadRequestUnstable,
  ): Promise<ProviderConfigReadResponseUnstable> {
    const raw = await this.conn.request(
      "_bcaip/unstable/providers/config/read",
      params,
    );
    return providerConfigReadResponseUnstableSchema.parse(
      raw,
    ) as ProviderConfigReadResponseUnstable;
  }

  async providersConfigStatusUnstable(
    params: ProviderConfigStatusRequestUnstable,
  ): Promise<ProviderConfigStatusResponseUnstable> {
    const raw = await this.conn.request(
      "_bcaip/unstable/providers/config/status",
      params,
    );
    return providerConfigStatusResponseUnstableSchema.parse(
      raw,
    ) as ProviderConfigStatusResponseUnstable;
  }

  async providersConfigSaveUnstable(
    params: ProviderConfigSaveRequestUnstable,
  ): Promise<ProviderConfigChangeResponseUnstable> {
    const raw = await this.conn.request(
      "_bcaip/unstable/providers/config/save",
      params,
    );
    return providerConfigChangeResponseUnstableSchema.parse(
      raw,
    ) as ProviderConfigChangeResponseUnstable;
  }

  async providersConfigDeleteUnstable(
    params: ProviderConfigDeleteRequestUnstable,
  ): Promise<ProviderConfigChangeResponseUnstable> {
    const raw = await this.conn.request(
      "_bcaip/unstable/providers/config/delete",
      params,
    );
    return providerConfigChangeResponseUnstableSchema.parse(
      raw,
    ) as ProviderConfigChangeResponseUnstable;
  }

  async providersConfigAuthenticateUnstable(
    params: ProviderConfigAuthenticateRequestUnstable,
  ): Promise<ProviderConfigChangeResponseUnstable> {
    const raw = await this.conn.request(
      "_bcaip/unstable/providers/config/authenticate",
      params,
    );
    return providerConfigChangeResponseUnstableSchema.parse(
      raw,
    ) as ProviderConfigChangeResponseUnstable;
  }

  async providersSecretsListUnstable(
    params: ProviderSecretsListRequestUnstable,
  ): Promise<ProviderSecretsListResponseUnstable> {
    const raw = await this.conn.request(
      "_bcaip/unstable/providers/secrets/list",
      params,
    );
    return providerSecretsListResponseUnstableSchema.parse(
      raw,
    ) as ProviderSecretsListResponseUnstable;
  }

  async providersSecretsDeleteUnstable(
    params: ProviderSecretDeleteRequestUnstable,
  ): Promise<void> {
    await this.conn.request("_bcaip/unstable/providers/secrets/delete", params);
  }

  async providersCanonicalModelInfoUnstable(
    params: CanonicalModelInfoRequestUnstable,
  ): Promise<CanonicalModelInfoResponseUnstable> {
    const raw = await this.conn.request(
      "_bcaip/unstable/providers/canonical-model-info",
      params,
    );
    return canonicalModelInfoResponseUnstableSchema.parse(
      raw,
    ) as CanonicalModelInfoResponseUnstable;
  }

  async preferencesReadUnstable(
    params: PreferencesReadRequestUnstable,
  ): Promise<PreferencesReadResponseUnstable> {
    const raw = await this.conn.request(
      "_bcaip/unstable/preferences/read",
      params,
    );
    return preferencesReadResponseUnstableSchema.parse(
      raw,
    ) as PreferencesReadResponseUnstable;
  }

  async preferencesSaveUnstable(
    params: PreferencesSaveRequestUnstable,
  ): Promise<void> {
    await this.conn.request("_bcaip/unstable/preferences/save", params);
  }

  async configReadUnstable(
    params: ConfigReadRequestUnstable,
  ): Promise<ConfigReadResponseUnstable> {
    const raw = await this.conn.request("_bcaip/unstable/config/read", params);
    return configReadResponseUnstableSchema.parse(
      raw,
    ) as ConfigReadResponseUnstable;
  }

  async configUpsertUnstable(
    params: ConfigUpsertRequestUnstable,
  ): Promise<void> {
    await this.conn.request("_bcaip/unstable/config/upsert", params);
  }

  async configRemoveUnstable(
    params: ConfigRemoveRequestUnstable,
  ): Promise<void> {
    await this.conn.request("_bcaip/unstable/config/remove", params);
  }

  async configReadAllUnstable(
    params: ConfigReadAllRequestUnstable,
  ): Promise<ConfigReadAllResponseUnstable> {
    const raw = await this.conn.request(
      "_bcaip/unstable/config/read-all",
      params,
    );
    return configReadAllResponseUnstableSchema.parse(
      raw,
    ) as ConfigReadAllResponseUnstable;
  }

  async defaultsReadUnstable(
    params: DefaultsReadRequestUnstable,
  ): Promise<DefaultsReadResponseUnstable> {
    const raw = await this.conn.request(
      "_bcaip/unstable/defaults/read",
      params,
    );
    return defaultsReadResponseUnstableSchema.parse(
      raw,
    ) as DefaultsReadResponseUnstable;
  }

  async defaultsSaveUnstable(
    params: DefaultsSaveRequestUnstable,
  ): Promise<DefaultsReadResponseUnstable> {
    const raw = await this.conn.request(
      "_bcaip/unstable/defaults/save",
      params,
    );
    return defaultsReadResponseUnstableSchema.parse(
      raw,
    ) as DefaultsReadResponseUnstable;
  }

  async defaultsClearUnstable(
    params: DefaultsClearRequestUnstable,
  ): Promise<DefaultsReadResponseUnstable> {
    const raw = await this.conn.request(
      "_bcaip/unstable/defaults/clear",
      params,
    );
    return defaultsReadResponseUnstableSchema.parse(
      raw,
    ) as DefaultsReadResponseUnstable;
  }

  async onboardingImportScanUnstable(
    params: OnboardingImportScanRequestUnstable,
  ): Promise<OnboardingImportScanResponseUnstable> {
    const raw = await this.conn.request(
      "_bcaip/unstable/onboarding/import/scan",
      params,
    );
    return onboardingImportScanResponseUnstableSchema.parse(
      raw,
    ) as OnboardingImportScanResponseUnstable;
  }

  async onboardingImportApplyUnstable(
    params: OnboardingImportApplyRequestUnstable,
  ): Promise<OnboardingImportApplyResponseUnstable> {
    const raw = await this.conn.request(
      "_bcaip/unstable/onboarding/import/apply",
      params,
    );
    return onboardingImportApplyResponseUnstableSchema.parse(
      raw,
    ) as OnboardingImportApplyResponseUnstable;
  }

  async sessionExportUnstable(
    params: ExportSessionRequestUnstable,
  ): Promise<ExportSessionResponseUnstable> {
    const raw = await this.conn.request(
      "_bcaip/unstable/session/export",
      params,
    );
    return exportSessionResponseUnstableSchema.parse(
      raw,
    ) as ExportSessionResponseUnstable;
  }

  async sessionImportUnstable(
    params: ImportSessionRequestUnstable,
  ): Promise<ImportSessionResponseUnstable> {
    const raw = await this.conn.request(
      "_bcaip/unstable/session/import",
      params,
    );
    return importSessionResponseUnstableSchema.parse(
      raw,
    ) as ImportSessionResponseUnstable;
  }

  async recipesEncodeUnstable(
    params: EncodeRecipeRequestUnstable,
  ): Promise<EncodeRecipeResponseUnstable> {
    const raw = await this.conn.request(
      "_bcaip/unstable/recipes/encode",
      params,
    );
    return encodeRecipeResponseUnstableSchema.parse(
      raw,
    ) as EncodeRecipeResponseUnstable;
  }

  async recipesDecodeUnstable(
    params: DecodeRecipeRequestUnstable,
  ): Promise<DecodeRecipeResponseUnstable> {
    const raw = await this.conn.request(
      "_bcaip/unstable/recipes/decode",
      params,
    );
    return decodeRecipeResponseUnstableSchema.parse(
      raw,
    ) as DecodeRecipeResponseUnstable;
  }

  async recipesScanUnstable(
    params: ScanRecipeRequestUnstable,
  ): Promise<ScanRecipeResponseUnstable> {
    const raw = await this.conn.request("_bcaip/unstable/recipes/scan", params);
    return scanRecipeResponseUnstableSchema.parse(
      raw,
    ) as ScanRecipeResponseUnstable;
  }

  async recipesListUnstable(
    params: ListRecipesRequestUnstable,
  ): Promise<ListRecipesResponseUnstable> {
    const raw = await this.conn.request("_bcaip/unstable/recipes/list", params);
    return listRecipesResponseUnstableSchema.parse(
      raw,
    ) as ListRecipesResponseUnstable;
  }

  async recipesDeleteUnstable(
    params: DeleteRecipeRequestUnstable,
  ): Promise<void> {
    await this.conn.request("_bcaip/unstable/recipes/delete", params);
  }

  async recipesScheduleUnstable(
    params: ScheduleRecipeRequestUnstable,
  ): Promise<void> {
    await this.conn.request("_bcaip/unstable/recipes/schedule", params);
  }

  async recipesSlashCommandUnstable(
    params: SetRecipeSlashCommandRequestUnstable,
  ): Promise<void> {
    await this.conn.request("_bcaip/unstable/recipes/slash-command", params);
  }

  async recipesSaveUnstable(
    params: SaveRecipeRequestUnstable,
  ): Promise<SaveRecipeResponseUnstable> {
    const raw = await this.conn.request("_bcaip/unstable/recipes/save", params);
    return saveRecipeResponseUnstableSchema.parse(
      raw,
    ) as SaveRecipeResponseUnstable;
  }

  async recipesParseUnstable(
    params: ParseRecipeRequestUnstable,
  ): Promise<ParseRecipeResponseUnstable> {
    const raw = await this.conn.request(
      "_bcaip/unstable/recipes/parse",
      params,
    );
    return parseRecipeResponseUnstableSchema.parse(
      raw,
    ) as ParseRecipeResponseUnstable;
  }

  async recipesToYamlUnstable(
    params: RecipeToYamlRequestUnstable,
  ): Promise<RecipeToYamlResponseUnstable> {
    const raw = await this.conn.request(
      "_bcaip/unstable/recipes/to-yaml",
      params,
    );
    return recipeToYamlResponseUnstableSchema.parse(
      raw,
    ) as RecipeToYamlResponseUnstable;
  }

  async schedulesListUnstable(
    params: ListSchedulesRequestUnstable,
  ): Promise<ListSchedulesResponseUnstable> {
    const raw = await this.conn.request(
      "_bcaip/unstable/schedules/list",
      params,
    );
    return listSchedulesResponseUnstableSchema.parse(
      raw,
    ) as ListSchedulesResponseUnstable;
  }

  async schedulesSessionsListUnstable(
    params: ListScheduleSessionsRequestUnstable,
  ): Promise<ListScheduleSessionsResponseUnstable> {
    const raw = await this.conn.request(
      "_bcaip/unstable/schedules/sessions/list",
      params,
    );
    return listScheduleSessionsResponseUnstableSchema.parse(
      raw,
    ) as ListScheduleSessionsResponseUnstable;
  }

  async schedulesCreateUnstable(
    params: CreateScheduleRequestUnstable,
  ): Promise<CreateScheduleResponseUnstable> {
    const raw = await this.conn.request(
      "_bcaip/unstable/schedules/create",
      params,
    );
    return createScheduleResponseUnstableSchema.parse(
      raw,
    ) as CreateScheduleResponseUnstable;
  }

  async schedulesDeleteUnstable(
    params: DeleteScheduleRequestUnstable,
  ): Promise<void> {
    await this.conn.request("_bcaip/unstable/schedules/delete", params);
  }

  async schedulesPauseUnstable(
    params: PauseScheduleRequestUnstable,
  ): Promise<void> {
    await this.conn.request("_bcaip/unstable/schedules/pause", params);
  }

  async schedulesUnpauseUnstable(
    params: UnpauseScheduleRequestUnstable,
  ): Promise<void> {
    await this.conn.request("_bcaip/unstable/schedules/unpause", params);
  }

  async schedulesUpdateUnstable(
    params: UpdateScheduleRequestUnstable,
  ): Promise<UpdateScheduleResponseUnstable> {
    const raw = await this.conn.request(
      "_bcaip/unstable/schedules/update",
      params,
    );
    return updateScheduleResponseUnstableSchema.parse(
      raw,
    ) as UpdateScheduleResponseUnstable;
  }

  async schedulesRunNowUnstable(
    params: RunScheduleNowRequestUnstable,
  ): Promise<RunScheduleNowResponseUnstable> {
    const raw = await this.conn.request(
      "_bcaip/unstable/schedules/run-now",
      params,
    );
    return runScheduleNowResponseUnstableSchema.parse(
      raw,
    ) as RunScheduleNowResponseUnstable;
  }

  async schedulesRunningJobKillUnstable(
    params: KillRunningJobRequestUnstable,
  ): Promise<KillRunningJobResponseUnstable> {
    const raw = await this.conn.request(
      "_bcaip/unstable/schedules/running-job/kill",
      params,
    );
    return killRunningJobResponseUnstableSchema.parse(
      raw,
    ) as KillRunningJobResponseUnstable;
  }

  async schedulesRunningJobInspectUnstable(
    params: InspectRunningJobRequestUnstable,
  ): Promise<InspectRunningJobResponseUnstable> {
    const raw = await this.conn.request(
      "_bcaip/unstable/schedules/running-job/inspect",
      params,
    );
    return inspectRunningJobResponseUnstableSchema.parse(
      raw,
    ) as InspectRunningJobResponseUnstable;
  }

  async sessionInfoUnstable(
    params: GetSessionInfoRequestUnstable,
  ): Promise<GetSessionInfoResponseUnstable> {
    const raw = await this.conn.request("_bcaip/unstable/session/info", params);
    return getSessionInfoResponseUnstableSchema.parse(
      raw,
    ) as GetSessionInfoResponseUnstable;
  }

  async sessionConversationTruncateUnstable(
    params: TruncateSessionConversationRequestUnstable,
  ): Promise<void> {
    await this.conn.request(
      "_bcaip/unstable/session/conversation/truncate",
      params,
    );
  }

  async sessionProjectUpdateUnstable(
    params: UpdateSessionProjectRequestUnstable,
  ): Promise<void> {
    await this.conn.request("_bcaip/unstable/session/project/update", params);
  }

  async sessionRenameUnstable(
    params: RenameSessionRequestUnstable,
  ): Promise<void> {
    await this.conn.request("_bcaip/unstable/session/rename", params);
  }

  async sessionArchiveUnstable(
    params: ArchiveSessionRequestUnstable,
  ): Promise<void> {
    await this.conn.request("_bcaip/unstable/session/archive", params);
  }

  async sessionUnarchiveUnstable(
    params: UnarchiveSessionRequestUnstable,
  ): Promise<void> {
    await this.conn.request("_bcaip/unstable/session/unarchive", params);
  }

  async sourcesCreateUnstable(
    params: CreateSourceRequestUnstable,
  ): Promise<CreateSourceResponseUnstable> {
    const raw = await this.conn.request(
      "_bcaip/unstable/sources/create",
      params,
    );
    return createSourceResponseUnstableSchema.parse(
      raw,
    ) as CreateSourceResponseUnstable;
  }

  async sourcesListUnstable(
    params: ListSourcesRequestUnstable,
  ): Promise<ListSourcesResponseUnstable> {
    const raw = await this.conn.request("_bcaip/unstable/sources/list", params);
    return listSourcesResponseUnstableSchema.parse(
      raw,
    ) as ListSourcesResponseUnstable;
  }

  async agentMentionsListUnstable(
    params: ListAgentMentionsRequestUnstable,
  ): Promise<ListAgentMentionsResponseUnstable> {
    const raw = await this.conn.request(
      "_bcaip/unstable/agent-mentions/list",
      params,
    );
    return listAgentMentionsResponseUnstableSchema.parse(
      raw,
    ) as ListAgentMentionsResponseUnstable;
  }

  async slashCommandsListUnstable(
    params: ListSlashCommandsRequestUnstable,
  ): Promise<ListSlashCommandsResponseUnstable> {
    const raw = await this.conn.request(
      "_bcaip/unstable/slash-commands/list",
      params,
    );
    return listSlashCommandsResponseUnstableSchema.parse(
      raw,
    ) as ListSlashCommandsResponseUnstable;
  }

  async sourcesUpdateUnstable(
    params: UpdateSourceRequestUnstable,
  ): Promise<UpdateSourceResponseUnstable> {
    const raw = await this.conn.request(
      "_bcaip/unstable/sources/update",
      params,
    );
    return updateSourceResponseUnstableSchema.parse(
      raw,
    ) as UpdateSourceResponseUnstable;
  }

  async sourcesDeleteUnstable(
    params: DeleteSourceRequestUnstable,
  ): Promise<void> {
    await this.conn.request("_bcaip/unstable/sources/delete", params);
  }

  async sourcesExportUnstable(
    params: ExportSourceRequestUnstable,
  ): Promise<ExportSourceResponseUnstable> {
    const raw = await this.conn.request(
      "_bcaip/unstable/sources/export",
      params,
    );
    return exportSourceResponseUnstableSchema.parse(
      raw,
    ) as ExportSourceResponseUnstable;
  }

  async sourcesImportUnstable(
    params: ImportSourcesRequestUnstable,
  ): Promise<ImportSourcesResponseUnstable> {
    const raw = await this.conn.request(
      "_bcaip/unstable/sources/import",
      params,
    );
    return importSourcesResponseUnstableSchema.parse(
      raw,
    ) as ImportSourcesResponseUnstable;
  }

  async dictationTranscribeUnstable(
    params: DictationTranscribeRequestUnstable,
  ): Promise<DictationTranscribeResponseUnstable> {
    const raw = await this.conn.request(
      "_bcaip/unstable/dictation/transcribe",
      params,
    );
    return dictationTranscribeResponseUnstableSchema.parse(
      raw,
    ) as DictationTranscribeResponseUnstable;
  }

  async dictationConfigUnstable(
    params: DictationConfigRequestUnstable,
  ): Promise<DictationConfigResponseUnstable> {
    const raw = await this.conn.request(
      "_bcaip/unstable/dictation/config",
      params,
    );
    return dictationConfigResponseUnstableSchema.parse(
      raw,
    ) as DictationConfigResponseUnstable;
  }

  async dictationModelsListUnstable(
    params: DictationModelsListRequestUnstable,
  ): Promise<DictationModelsListResponseUnstable> {
    const raw = await this.conn.request(
      "_bcaip/unstable/dictation/models/list",
      params,
    );
    return dictationModelsListResponseUnstableSchema.parse(
      raw,
    ) as DictationModelsListResponseUnstable;
  }

  async dictationModelsDownloadUnstable(
    params: DictationModelDownloadRequestUnstable,
  ): Promise<void> {
    await this.conn.request(
      "_bcaip/unstable/dictation/models/download",
      params,
    );
  }

  async dictationModelsDownloadProgressUnstable(
    params: DictationModelDownloadProgressRequestUnstable,
  ): Promise<DictationModelDownloadProgressResponseUnstable> {
    const raw = await this.conn.request(
      "_bcaip/unstable/dictation/models/download/progress",
      params,
    );
    return dictationModelDownloadProgressResponseUnstableSchema.parse(
      raw,
    ) as DictationModelDownloadProgressResponseUnstable;
  }

  async dictationModelsCancelUnstable(
    params: DictationModelCancelRequestUnstable,
  ): Promise<void> {
    await this.conn.request("_bcaip/unstable/dictation/models/cancel", params);
  }

  async dictationModelsDeleteUnstable(
    params: DictationModelDeleteRequestUnstable,
  ): Promise<void> {
    await this.conn.request("_bcaip/unstable/dictation/models/delete", params);
  }

  async localInferenceModelsListUnstable(
    params: LocalInferenceModelsListRequestUnstable,
  ): Promise<LocalInferenceModelsListResponseUnstable> {
    const raw = await this.conn.request(
      "_bcaip/unstable/local-inference/models/list",
      params,
    );
    return localInferenceModelsListResponseUnstableSchema.parse(
      raw,
    ) as LocalInferenceModelsListResponseUnstable;
  }

  async localInferenceModelsDownloadUnstable(
    params: LocalInferenceModelDownloadRequestUnstable,
  ): Promise<LocalInferenceModelDownloadResponseUnstable> {
    const raw = await this.conn.request(
      "_bcaip/unstable/local-inference/models/download",
      params,
    );
    return localInferenceModelDownloadResponseUnstableSchema.parse(
      raw,
    ) as LocalInferenceModelDownloadResponseUnstable;
  }

  async localInferenceModelsDownloadProgressUnstable(
    params: LocalInferenceModelDownloadProgressRequestUnstable,
  ): Promise<LocalInferenceModelDownloadProgressResponseUnstable> {
    const raw = await this.conn.request(
      "_bcaip/unstable/local-inference/models/download/progress",
      params,
    );
    return localInferenceModelDownloadProgressResponseUnstableSchema.parse(
      raw,
    ) as LocalInferenceModelDownloadProgressResponseUnstable;
  }

  async localInferenceModelsDownloadCancelUnstable(
    params: LocalInferenceModelDownloadCancelRequestUnstable,
  ): Promise<void> {
    await this.conn.request(
      "_bcaip/unstable/local-inference/models/download/cancel",
      params,
    );
  }

  async localInferenceModelsDeleteUnstable(
    params: LocalInferenceModelDeleteRequestUnstable,
  ): Promise<void> {
    await this.conn.request(
      "_bcaip/unstable/local-inference/models/delete",
      params,
    );
  }

  async localInferenceModelsEvictUnstable(
    params: LocalInferenceModelEvictRequestUnstable,
  ): Promise<void> {
    await this.conn.request(
      "_bcaip/unstable/local-inference/models/evict",
      params,
    );
  }

  async localInferenceModelsSettingsReadUnstable(
    params: LocalInferenceModelSettingsReadRequestUnstable,
  ): Promise<LocalInferenceModelSettingsReadResponseUnstable> {
    const raw = await this.conn.request(
      "_bcaip/unstable/local-inference/models/settings/read",
      params,
    );
    return localInferenceModelSettingsReadResponseUnstableSchema.parse(
      raw,
    ) as LocalInferenceModelSettingsReadResponseUnstable;
  }

  async localInferenceModelsSettingsUpdateUnstable(
    params: LocalInferenceModelSettingsUpdateRequestUnstable,
  ): Promise<LocalInferenceModelSettingsUpdateResponseUnstable> {
    const raw = await this.conn.request(
      "_bcaip/unstable/local-inference/models/settings/update",
      params,
    );
    return localInferenceModelSettingsUpdateResponseUnstableSchema.parse(
      raw,
    ) as LocalInferenceModelSettingsUpdateResponseUnstable;
  }

  async localInferenceHuggingfaceSearchUnstable(
    params: LocalInferenceHuggingFaceSearchRequestUnstable,
  ): Promise<LocalInferenceHuggingFaceSearchResponseUnstable> {
    const raw = await this.conn.request(
      "_bcaip/unstable/local-inference/huggingface/search",
      params,
    );
    return localInferenceHuggingFaceSearchResponseUnstableSchema.parse(
      raw,
    ) as LocalInferenceHuggingFaceSearchResponseUnstable;
  }

  async localInferenceHuggingfaceRepoVariantsUnstable(
    params: LocalInferenceHuggingFaceRepoVariantsRequestUnstable,
  ): Promise<LocalInferenceHuggingFaceRepoVariantsResponseUnstable> {
    const raw = await this.conn.request(
      "_bcaip/unstable/local-inference/huggingface/repo/variants",
      params,
    );
    return localInferenceHuggingFaceRepoVariantsResponseUnstableSchema.parse(
      raw,
    ) as LocalInferenceHuggingFaceRepoVariantsResponseUnstable;
  }

  async localInferenceChatTemplatesBuiltinListUnstable(
    params: LocalInferenceBuiltinChatTemplatesListRequestUnstable,
  ): Promise<LocalInferenceBuiltinChatTemplatesListResponseUnstable> {
    const raw = await this.conn.request(
      "_bcaip/unstable/local-inference/chat-templates/builtin/list",
      params,
    );
    return localInferenceBuiltinChatTemplatesListResponseUnstableSchema.parse(
      raw,
    ) as LocalInferenceBuiltinChatTemplatesListResponseUnstable;
  }
}
