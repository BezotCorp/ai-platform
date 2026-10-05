import type { UpdateCustomProviderRequest } from '../../../../../../../types/updateCustomProviderRequest';

export interface CustomProviderFormProps {
  onSubmit: (data: UpdateCustomProviderRequest) => void | Promise<void>;
  onCancel: () => void;
  onDelete?: () => Promise<void>;
  isActiveProvider?: boolean;
  initialData: UpdateCustomProviderRequest | null;
  isEditable?: boolean;
}
