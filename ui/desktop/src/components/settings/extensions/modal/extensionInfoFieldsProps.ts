export interface ExtensionInfoFieldsProps {
  name: string;
  type: 'stdio' | 'streamable_http' | 'builtin';
  description: string;
  onChange: (key: string, value: string) => void;
  submitAttempted: boolean;
}
