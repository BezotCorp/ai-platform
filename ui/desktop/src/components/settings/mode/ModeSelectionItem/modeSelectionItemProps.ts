import type { BcaipMode } from '../ModeSelectionItem/bcaipMode';

export interface ModeSelectionItemProps {
  currentMode: string;
  mode: BcaipMode;
  showDescription: boolean;
  isApproveModeConfigure: boolean;
  handleModeChange: (newMode: string) => void;
}
