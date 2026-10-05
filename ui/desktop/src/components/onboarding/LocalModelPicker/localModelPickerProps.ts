export interface LocalModelPickerProps {
  onConfigured: (providerName: string, modelId: string) => void | Promise<void>;
}
