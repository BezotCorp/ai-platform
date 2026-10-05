export interface InlineEditTextProps {
  value: string;
  onSave: (newValue: string) => Promise<void>;
  maxLength?: number;
  placeholder?: string;
  disabled?: boolean;
  className?: string;
  editClassName?: string;
  onEditStart?: () => void;
  onEditEnd?: () => void;
  allowEmpty?: boolean;
  singleClickEdit?: boolean;
}
