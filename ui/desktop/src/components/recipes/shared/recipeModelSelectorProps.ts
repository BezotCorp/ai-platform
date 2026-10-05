export interface RecipeModelSelectorProps {
  selectedProvider?: string;
  selectedModel?: string;
  onProviderChange: (provider: string | undefined) => void;
  onModelChange: (model: string | undefined) => void;
}
