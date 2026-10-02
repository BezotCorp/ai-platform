import type { MethodMeta } from './method_meta.ts';

export interface Meta {
  methods: MethodMeta[];
  notifications?: unknown[];
  agentRequests?: unknown[];
}
