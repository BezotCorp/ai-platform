import type { ProviderTemplateDto } from '@aaif/goose-acp-client';

export interface ProviderCatalogPickerProps {
  onSelect: (template: ProviderTemplateDto) => void;
  onCancel: () => void;
  embedded?: boolean;
}
