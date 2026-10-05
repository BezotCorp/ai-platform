export interface HeadersSectionProps {
  headers: { key: string; value: string; isEdited?: boolean }[];
  onAdd: (key: string, value: string) => void;
  onRemove: (index: number) => void;
  onChange: (index: number, field: 'key' | 'value', value: string) => void;
  submitAttempted: boolean;
  onPendingInputChange: (
    hasPendingInput: boolean,
    pendingHeader: { key: string; value: string } | null
  ) => void;
}
