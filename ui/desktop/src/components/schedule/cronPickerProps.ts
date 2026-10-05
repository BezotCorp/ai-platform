import type { ScheduledJobDto } from '@aaif/goose-acp-client';

export interface CronPickerProps {
  schedule: ScheduledJobDto | null;
  onChange: (cron: string) => void;
  isValid: (valid: boolean) => void;
}
