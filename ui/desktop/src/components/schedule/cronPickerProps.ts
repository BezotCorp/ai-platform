import type { ScheduledJobDto } from '@bezotcorp/bcaip-acp-client';

export interface CronPickerProps {
  schedule: ScheduledJobDto | null;
  onChange: (cron: string) => void;
  isValid: (valid: boolean) => void;
}
