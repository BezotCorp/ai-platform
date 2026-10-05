import type { Recipe } from '../../../recipe';

export type NewSchedulePayload =
  | {
      sourceType: 'file' | 'deeplink';
      id: string;
      recipe: Recipe;
      cron: string;
    }
  | {
      sourceType: 'saved';
      recipeId: string;
      cron: string;
    };
