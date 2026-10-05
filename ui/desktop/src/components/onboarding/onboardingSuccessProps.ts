export interface OnboardingSuccessProps {
  providerName: string;
  onFinish: (telemetryEnabled: boolean) => void;
}
