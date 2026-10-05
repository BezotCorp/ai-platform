import type { RecipeExtension } from '../../../../recipe';

export interface RecipeExtensionSelectorProps {
  selectedExtensions: RecipeExtension[];
  onExtensionsChange: (extensions: RecipeExtension[]) => void;
}
