import { AppDate } from '../utils/appDate';
import type { ExtensionData } from './extensionData';
import type { BcaipMode } from './bcaipMode';
import type { Message } from './message';
import type { ModelConfig } from './modelConfig';
import type { SessionChanges } from './sessionChanges';
import type { SessionData } from './sessionData';
import type { SessionType } from './sessionType';
import type { Usage } from './usage';
import type { Recipe } from '../recipe';

/**
 * Application representation of a BCAIP session.
 *
 * Serialized transport values enter through `SessionData`. Dates are converted
 * to `AppDate` immediately and remain strongly typed inside the application.
 */
export class Session {
  private accumulatedCost?: number | null;
  private accumulatedUsage?: Usage;
  private archivedAt?: AppDate | null;
  private conversationValue?: Message[] | null;
  private createdAt: AppDate;
  private extensionData: ExtensionData;
  private bcaipMode?: BcaipMode;
  private sessionId: string;
  private lastMessageAt?: AppDate | null;
  private lastMessageSnippet?: string | null;
  private messageCount: number;
  private modelConfig?: ModelConfig | null;
  private sessionName: string;
  private projectId?: string | null;
  private providerName?: string | null;
  private recipeValue?: Recipe | null;
  private scheduleId?: string | null;
  private sessionType?: SessionType;
  private updatedAt: AppDate;
  private usageValue?: Usage;
  private userRecipeValues?: Record<string, string> | null;
  private userSetName?: boolean;
  private workingDir: string;

  public constructor(data: SessionData) {
    this.accumulatedCost = data.accumulated_cost;
    this.accumulatedUsage = data.accumulated_usage;
    this.archivedAt = Session.parseOptionalDate(data.archived_at);
    this.conversationValue = data.conversation;
    this.createdAt = AppDate.fromString(data.created_at);
    this.extensionData = data.extension_data;
    this.bcaipMode = data.bcaip_mode;
    this.sessionId = data.id;
    this.lastMessageAt = Session.parseOptionalDate(data.last_message_at);
    this.lastMessageSnippet = data.last_message_snippet;
    this.messageCount = data.message_count;
    this.modelConfig = data.model_config;
    this.sessionName = data.name;
    this.projectId = data.project_id;
    this.providerName = data.provider_name;
    this.recipeValue = data.recipe;
    this.scheduleId = data.schedule_id;
    this.sessionType = data.session_type;
    this.updatedAt = AppDate.fromString(data.updated_at);
    this.usageValue = data.usage;
    this.userRecipeValues = data.user_recipe_values;
    this.userSetName = data.user_set_name;
    this.workingDir = data.working_dir;
  }

  /**
   * Creates a domain session from serialized session data.
   */
  public static fromData(data: SessionData): Session {
    return new Session(data);
  }

  /**
   * Creates a new session with selected domain values changed.
   */
  public with(changes: SessionChanges): Session {
    const { created_at, updated_at, last_message_at, archived_at, ...plainChanges } = changes;

    const data: SessionData = {
      ...this.toData(),
      ...plainChanges,
    };

    if (created_at !== undefined) {
      data.created_at = created_at.toISOString();
    }

    if (updated_at !== undefined) {
      data.updated_at = updated_at.toISOString();
    }

    if (last_message_at !== undefined) {
      data.last_message_at = last_message_at === null ? null : last_message_at.toISOString();
    }

    if (archived_at !== undefined) {
      data.archived_at = archived_at === null ? null : archived_at.toISOString();
    }

    return new Session(data);
  }

  /**
   * Converts the domain session back to serializable transport data.
   */
  public toData(): SessionData {
    return {
      accumulated_cost: this.accumulatedCost,
      accumulated_usage: this.accumulatedUsage,
      archived_at: Session.serializeOptionalDate(this.archivedAt),
      conversation: this.conversationValue,
      created_at: this.createdAt.toISOString(),
      extension_data: this.extensionData,
      bcaip_mode: this.bcaipMode,
      id: this.sessionId,
      last_message_at: Session.serializeOptionalDate(this.lastMessageAt),
      last_message_snippet: this.lastMessageSnippet,
      message_count: this.messageCount,
      model_config: this.modelConfig,
      name: this.sessionName,
      project_id: this.projectId,
      provider_name: this.providerName,
      recipe: this.recipeValue,
      schedule_id: this.scheduleId,
      session_type: this.sessionType,
      updated_at: this.updatedAt.toISOString(),
      usage: this.usageValue,
      user_recipe_values: this.userRecipeValues,
      user_set_name: this.userSetName,
      working_dir: this.workingDir,
    };
  }

  public get accumulated_cost(): number | null | undefined {
    return this.accumulatedCost;
  }

  public set accumulated_cost(value: number | null | undefined) {
    this.accumulatedCost = value;
  }

  public get accumulated_usage(): Usage | undefined {
    return this.accumulatedUsage;
  }

  public set accumulated_usage(value: Usage | undefined) {
    this.accumulatedUsage = value;
  }

  public get archived_at(): AppDate | null | undefined {
    return this.archivedAt;
  }

  public set archived_at(value: AppDate | null | undefined) {
    this.archivedAt = value;
  }

  public get conversation(): Message[] | null | undefined {
    return this.conversationValue;
  }

  public set conversation(value: Message[] | null | undefined) {
    this.conversationValue = value;
  }

  public get created_at(): AppDate {
    return this.createdAt;
  }

  public set created_at(value: AppDate) {
    this.createdAt = value;
  }

  public get extension_data(): ExtensionData {
    return this.extensionData;
  }

  public set extension_data(value: ExtensionData) {
    this.extensionData = value;
  }

  public get bcaip_mode(): BcaipMode | undefined {
    return this.bcaipMode;
  }

  public set bcaip_mode(value: BcaipMode | undefined) {
    this.bcaipMode = value;
  }

  public get id(): string {
    return this.sessionId;
  }

  public set id(value: string) {
    this.sessionId = value;
  }

  public get last_message_at(): AppDate | null | undefined {
    return this.lastMessageAt;
  }

  public set last_message_at(value: AppDate | null | undefined) {
    this.lastMessageAt = value;
  }

  public get last_message_snippet(): string | null | undefined {
    return this.lastMessageSnippet;
  }

  public set last_message_snippet(value: string | null | undefined) {
    this.lastMessageSnippet = value;
  }

  public get message_count(): number {
    return this.messageCount;
  }

  public set message_count(value: number) {
    this.messageCount = value;
  }

  public get model_config(): ModelConfig | null | undefined {
    return this.modelConfig;
  }

  public set model_config(value: ModelConfig | null | undefined) {
    this.modelConfig = value;
  }

  public get name(): string {
    return this.sessionName;
  }

  public set name(value: string) {
    this.sessionName = value;
  }

  public get project_id(): string | null | undefined {
    return this.projectId;
  }

  public set project_id(value: string | null | undefined) {
    this.projectId = value;
  }

  public get provider_name(): string | null | undefined {
    return this.providerName;
  }

  public set provider_name(value: string | null | undefined) {
    this.providerName = value;
  }

  public get recipe(): Recipe | null | undefined {
    return this.recipeValue;
  }

  public set recipe(value: Recipe | null | undefined) {
    this.recipeValue = value;
  }

  public get schedule_id(): string | null | undefined {
    return this.scheduleId;
  }

  public set schedule_id(value: string | null | undefined) {
    this.scheduleId = value;
  }

  public get session_type(): SessionType | undefined {
    return this.sessionType;
  }

  public set session_type(value: SessionType | undefined) {
    this.sessionType = value;
  }

  public get updated_at(): AppDate {
    return this.updatedAt;
  }

  public set updated_at(value: AppDate) {
    this.updatedAt = value;
  }

  public get usage(): Usage | undefined {
    return this.usageValue;
  }

  public set usage(value: Usage | undefined) {
    this.usageValue = value;
  }

  public get user_recipe_values(): Record<string, string> | null | undefined {
    return this.userRecipeValues;
  }

  public set user_recipe_values(value: Record<string, string> | null | undefined) {
    this.userRecipeValues = value;
  }

  public get user_set_name(): boolean | undefined {
    return this.userSetName;
  }

  public set user_set_name(value: boolean | undefined) {
    this.userSetName = value;
  }

  public get working_dir(): string {
    return this.workingDir;
  }

  public set working_dir(value: string) {
    this.workingDir = value;
  }

  private static parseOptionalDate(value: string | null | undefined): AppDate | null | undefined {
    if (value === null) {
      return null;
    }

    if (value === undefined) {
      return undefined;
    }

    return AppDate.fromString(value);
  }

  private static serializeOptionalDate(
    value: AppDate | null | undefined
  ): string | null | undefined {
    if (value === null) {
      return null;
    }

    if (value === undefined) {
      return undefined;
    }

    return value.toISOString();
  }
}
