import type { Period } from './period';

export type CronParts = {
  period: Period;
  second: string;
  minute: string;
  hour24: number;
  dayOfWeek: string;
  dayOfMonth: string | null;
  month: string;
  quarterStartMonth: string;
  customCron: string;
};
