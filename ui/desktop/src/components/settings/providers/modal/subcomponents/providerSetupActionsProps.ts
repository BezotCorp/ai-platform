import { SyntheticEvent } from 'react';
import type { ConfigKey } from '../../../../../types/configKey';

export interface ProviderSetupActionsProps {
  onCancel: () => void;
  onSubmit: (e: SyntheticEvent) => void;
  onDelete?: () => void;
  showDeleteConfirmation?: boolean;
  onConfirmDelete?: () => void;
  onCancelDelete?: () => void;
  canDelete?: boolean;
  providerName?: string;
  primaryParameters?: ConfigKey[];
  isActiveProvider?: boolean; // Made optional with default false
}
