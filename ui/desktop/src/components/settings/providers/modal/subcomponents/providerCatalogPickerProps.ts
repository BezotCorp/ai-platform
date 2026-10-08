import type { ProviderTemplateDto } from '@bezotcorp/bcaip-acp-client';

export interface ProviderCatalogPickerProps {
  onSelect: (template: ProviderTemplateDto) => void;
  onCancel: () => void;
  embedded?: boolean;
}
