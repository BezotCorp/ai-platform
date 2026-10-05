export interface ConfigureApproveModeProps {
  onClose: () => void;
  handleModeChange: (newMode: string) => void;
  currentMode: string | null;
}
