import { AppDate } from '../utils/appDate';
import type { SessionType } from '../types/sessionType';
import type { SessionListItemChanges } from './sessionListItemChanges';
import type { SessionListItemData } from './sessionListItemData';

/**
 * Lightweight application representation of a session.
 *
 * Serialized ACP dates are converted to `AppDate` at construction time so
 * consumers never need to repeatedly parse transport strings.
 */
export class SessionListItem {
  private sessionId: string;
  private sessionName: string;
  private workingDirectory: string;
  private updatedAtValue: AppDate;
  private messageCountValue: number;
  private lastMessageAtValue?: AppDate;
  private createdAtValue: AppDate;
  private archivedAtValue?: AppDate;
  private projectIdValue?: string;
  private providerIdValue?: string;
  private modelIdValue?: string;
  private userSetNameValue?: boolean;
  private hasRecipeValue?: boolean;
  private sessionTypeValue?: SessionType;

  public constructor(data: SessionListItemData) {
    this.sessionId = data.id;
    this.sessionName = data.name;
    this.workingDirectory = data.workingDir;
    this.updatedAtValue = AppDate.fromString(data.updatedAt);
    this.messageCountValue = data.messageCount;
    this.lastMessageAtValue = SessionListItem.parseOptionalDate(data.lastMessageAt);
    this.createdAtValue = AppDate.fromString(data.createdAt);
    this.archivedAtValue = SessionListItem.parseOptionalDate(data.archivedAt);
    this.projectIdValue = data.projectId;
    this.providerIdValue = data.providerId;
    this.modelIdValue = data.modelId;
    this.userSetNameValue = data.userSetName;
    this.hasRecipeValue = data.hasRecipe;
    this.sessionTypeValue = data.sessionType;
  }

  public static fromData(data: SessionListItemData): SessionListItem {
    return new SessionListItem(data);
  }

  public with(changes: SessionListItemChanges): SessionListItem {
    const {
      updatedAt,
      lastMessageAt,
      createdAt,
      archivedAt,
      ...plainChanges
    } = changes;

    const data: SessionListItemData = {
      ...this.toData(),
      ...plainChanges,
    };

    if (updatedAt !== undefined) {
      data.updatedAt = updatedAt.toISOString();
    }

    if (lastMessageAt !== undefined) {
      data.lastMessageAt = lastMessageAt.toISOString();
    }

    if (createdAt !== undefined) {
      data.createdAt = createdAt.toISOString();
    }

    if (archivedAt !== undefined) {
      data.archivedAt = archivedAt.toISOString();
    }

    return new SessionListItem(data);
  }

  public toData(): SessionListItemData {
    return {
      id: this.sessionId,
      name: this.sessionName,
      workingDir: this.workingDirectory,
      updatedAt: this.updatedAtValue.toISOString(),
      messageCount: this.messageCountValue,
      lastMessageAt: this.lastMessageAtValue?.toISOString(),
      createdAt: this.createdAtValue.toISOString(),
      archivedAt: this.archivedAtValue?.toISOString(),
      projectId: this.projectIdValue,
      providerId: this.providerIdValue,
      modelId: this.modelIdValue,
      userSetName: this.userSetNameValue,
      hasRecipe: this.hasRecipeValue,
      sessionType: this.sessionTypeValue,
    };
  }

  /**
   * Returns the date that best represents the latest meaningful session
   * activity.
   */
  public get activityAt(): AppDate {
    return this.lastMessageAtValue ?? this.updatedAtValue;
  }

  public get id(): string {
    return this.sessionId;
  }

  public get name(): string {
    return this.sessionName;
  }

  public get workingDir(): string {
    return this.workingDirectory;
  }

  public get updatedAt(): AppDate {
    return this.updatedAtValue;
  }

  public get messageCount(): number {
    return this.messageCountValue;
  }

  public get lastMessageAt(): AppDate | undefined {
    return this.lastMessageAtValue;
  }

  public get createdAt(): AppDate {
    return this.createdAtValue;
  }

  public get archivedAt(): AppDate | undefined {
    return this.archivedAtValue;
  }

  public get projectId(): string | undefined {
    return this.projectIdValue;
  }

  public get providerId(): string | undefined {
    return this.providerIdValue;
  }

  public get modelId(): string | undefined {
    return this.modelIdValue;
  }

  public get userSetName(): boolean | undefined {
    return this.userSetNameValue;
  }

  public get hasRecipe(): boolean | undefined {
    return this.hasRecipeValue;
  }

  public get sessionType(): SessionType | undefined {
    return this.sessionTypeValue;
  }

  private static parseOptionalDate(value: string | undefined): AppDate | undefined {
    return value === undefined ? undefined : AppDate.fromString(value);
  }
}
