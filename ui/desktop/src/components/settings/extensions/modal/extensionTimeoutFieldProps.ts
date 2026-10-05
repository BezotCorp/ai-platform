export interface ExtensionTimeoutFieldProps {
  timeout: number;
  onChange: (key: string, value: string | number) => void;
  submitAttempted: boolean;
}
