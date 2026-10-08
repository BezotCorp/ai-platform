import type { ScheduledJobDto } from '@bezotcorp/bcaip-acp-client';
import type { NewSchedulePayload } from '../ScheduleModal/newSchedulePayload';

export interface ScheduleModalProps {
  isOpen: boolean;
  onClose: () => void;
  onSubmit: (payload: NewSchedulePayload | string) => Promise<void>;
  schedule: ScheduledJobDto | null;
  isLoadingExternally: boolean;
  apiErrorExternally: string | null;
  initialDeepLink: string | null;
}
