import type { SessionData } from './sessionData';

/**
 * Application representation of a Goose session.
 *
 * `Session` owns session state and provides controlled access to it instead of
 * exposing free-form object literals throughout the application.
 *
 * A session can be copied with changes through `with()` and converted back to
 * plain serializable data through `toData()`.
 */
export class Session {
  private data: SessionData;

  public constructor(data: SessionData) {
    this.data = { ...data };
  }

  /**
   * Creates a new session from serializable session data.
   */
  public static fromData(data: SessionData): Session {
    return new Session(data);
  }

  /**
   * Returns a new session containing the current state with selected fields
   * replaced.
   */
  public with(changes: Partial<SessionData>): Session {
    return new Session({
      ...this.data,
      ...changes,
    });
  }

  /**
   * Returns a plain copy of the session state.
   */
  public toData(): SessionData {
    return { ...this.data };
  }

  public get accumulated_cost(): SessionData['accumulated_cost'] {
    return this.data.accumulated_cost;
  }

  public set accumulated_cost(value: SessionData['accumulated_cost']) {
    this.data.accumulated_cost = value;
  }

  public get accumulated_usage(): SessionData['accumulated_usage'] {
    return this.data.accumulated_usage;
  }

  public set accumulated_usage(value: SessionData['accumulated_usage']) {
    this.data.accumulated_usage = value;
  }

  public get archived_at(): SessionData['archived_at'] {
    return this.data.archived_at;
  }

  public set archived_at(value: SessionData['archived_at']) {
    this.data.archived_at = value;
  }

  public get conversation(): SessionData['conversation'] {
    return this.data.conversation;
  }

  public set conversation(value: SessionData['conversation']) {
    this.data.conversation = value;
  }

  public get created_at(): string {
    return this.data.created_at;
  }

  public set created_at(value: string) {
    this.data.created_at = value;
  }

  public get extension_data(): SessionData['extension_data'] {
    return this.data.extension_data;
  }

  public set extension_data(value: SessionData['extension_data']) {
    this.data.extension_data = value;
  }

  public get goose_mode(): SessionData['goose_mode'] {
    return this.data.goose_mode;
  }

  public set goose_mode(value: SessionData['goose_mode']) {
    this.data.goose_mode = value;
  }

  public get id(): string {
    return this.data.id;
  }

  public set id(value: string) {
    this.data.id = value;
  }

  public get last_message_at(): SessionData['last_message_at'] {
    return this.data.last_message_at;
  }

  public set last_message_at(value: SessionData['last_message_at']) {
    this.data.last_message_at = value;
  }

  public get last_message_snippet(): SessionData['last_message_snippet'] {
    return this.data.last_message_snippet;
  }

  public set last_message_snippet(value: SessionData['last_message_snippet']) {
    this.data.last_message_snippet = value;
  }

  public get message_count(): number {
    return this.data.message_count;
  }

  public set message_count(value: number) {
    this.data.message_count = value;
  }

  public get model_config(): SessionData['model_config'] {
    return this.data.model_config;
  }

  public set model_config(value: SessionData['model_config']) {
    this.data.model_config = value;
  }

  public get name(): string {
    return this.data.name;
  }

  public set name(value: string) {
    this.data.name = value;
  }

  public get project_id(): SessionData['project_id'] {
    return this.data.project_id;
  }

  public set project_id(value: SessionData['project_id']) {
    this.data.project_id = value;
  }

  public get provider_name(): SessionData['provider_name'] {
    return this.data.provider_name;
  }

  public set provider_name(value: SessionData['provider_name']) {
    this.data.provider_name = value;
  }

  public get recipe(): SessionData['recipe'] {
    return this.data.recipe;
  }

  public set recipe(value: SessionData['recipe']) {
    this.data.recipe = value;
  }

  public get schedule_id(): SessionData['schedule_id'] {
    return this.data.schedule_id;
  }

  public set schedule_id(value: SessionData['schedule_id']) {
    this.data.schedule_id = value;
  }

  public get session_type(): SessionData['session_type'] {
    return this.data.session_type;
  }

  public set session_type(value: SessionData['session_type']) {
    this.data.session_type = value;
  }

  public get updated_at(): string {
    return this.data.updated_at;
  }

  public set updated_at(value: string) {
    this.data.updated_at = value;
  }

  public get usage(): SessionData['usage'] {
    return this.data.usage;
  }

  public set usage(value: SessionData['usage']) {
    this.data.usage = value;
  }

  public get user_recipe_values(): SessionData['user_recipe_values'] {
    return this.data.user_recipe_values;
  }

  public set user_recipe_values(value: SessionData['user_recipe_values']) {
    this.data.user_recipe_values = value;
  }

  public get user_set_name(): SessionData['user_set_name'] {
    return this.data.user_set_name;
  }

  public set user_set_name(value: SessionData['user_set_name']) {
    this.data.user_set_name = value;
  }

  public get working_dir(): string {
    return this.data.working_dir;
  }

  public set working_dir(value: string) {
    this.data.working_dir = value;
  }
}
