import type { View } from '../../../../utils/navigationUtils';

export type SwitchModelModalProps = {
  sessionId: string | null;
  onClose: () => void;
  setView: (view: View) => void;
  onModelSelected?: (model: string, provider: string) => void;
  initialProvider?: string | null;
  titleOverride?: string;
  sessionModel?: string | null;
  sessionProvider?: string | null;
};
