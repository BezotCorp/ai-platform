import type { ResponseStyle } from '../ResponseStyleSelectionItem/responseStyle';

export interface ResponseStyleSelectionItemProps {
  currentStyle: string;
  style: ResponseStyle;
  showDescription: boolean;
  handleStyleChange: (newStyle: string) => void;
}
