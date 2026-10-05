export type ConfiguredParameterState =
  | { status: 'uninitialized' }
  | {
      status: 'active';
      scopeId: string;
      values: Record<string, string>;
      sessionId?: string;
    }
  | { status: 'consumed' };
