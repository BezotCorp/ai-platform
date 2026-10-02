import { useState, useEffect } from 'react';
import { Button } from './ui/button';
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle } from './ui/dialog';
import { TELEMETRY_UI_ENABLED } from '../updates';
import { useConfig } from './ConfigContext';
import {
  trackTelemetryPreference,
  setTelemetryEnabled as setAnalyticsTelemetryEnabled,
} from '../utils/analyticsEvent';
import PrivacyInfoModal from './onboarding/PrivacyInfoModal';
import { defineMessages, useIntl } from '../i18n';
import type { NoMessageValues } from 'react-intl';

const i18n = defineMessages<{
  readonly heading: NoMessageValues;
  readonly description: NoMessageValues;
  readonly learnMore: NoMessageValues;
  readonly optIn: NoMessageValues;
  readonly optOut: NoMessageValues;
}>({
  heading: {
    id: 'telemetryConsentPrompt.heading',
    defaultMessage: 'Help improve goose',
  },
  description: {
    id: 'telemetryConsentPrompt.description',
    defaultMessage:
      'Would you like to share anonymous usage data to help improve goose? We never collect your conversations, code, or personal data.',
  },
  learnMore: {
    id: 'telemetryConsentPrompt.learnMore',
    defaultMessage: 'Learn more',
  },
  optIn: {
    id: 'telemetryConsentPrompt.optIn',
    defaultMessage: 'Yes, share anonymous usage data',
  },
  optOut: {
    id: 'telemetryConsentPrompt.optOut',
    defaultMessage: 'No thanks',
  },
});

const TELEMETRY_CONFIG_KEY = 'GOOSE_TELEMETRY_ENABLED';

export default function TelemetryConsentPrompt(): React.JSX.Element | null {
  const intl = useIntl();
  const { read, upsert } = useConfig();
  const [showPrompt, setShowPrompt] = useState<boolean>(false);
  const [showPrivacyInfo, setShowPrivacyInfo] = useState<boolean>(false);
  const [isSubmitting, setIsSubmitting] = useState<boolean>(false);

  useEffect(() => {
    if (!TELEMETRY_UI_ENABLED) return;

    void (async (): Promise<void> => {
      try {
        const provider: unknown = await read('GOOSE_PROVIDER', false);
        if (!provider || provider === '') return;

        const telemetryValue: unknown = await read(TELEMETRY_CONFIG_KEY, false);
        if (telemetryValue === null) {
          setShowPrompt(true);
        }
      } catch (error) {
        console.error('Failed to check telemetry config:', error);
      }
    })();
  }, [read]);

  const handleChoice = async (enabled: boolean): Promise<void> => {
    setIsSubmitting(true);
    try {
      await upsert(TELEMETRY_CONFIG_KEY, enabled, false);
      trackTelemetryPreference(enabled, 'modal');
      setAnalyticsTelemetryEnabled(enabled);
    } catch (error) {
      console.error('Failed to save telemetry preference:', error);
    } finally {
      setShowPrompt(false);
      setIsSubmitting(false);
    }
  };

  if (!showPrompt) return null;

  return (
    <>
      <Dialog
        open
        onOpenChange={(open: boolean): void => {
          if (!open) setShowPrompt(false);
        }}
      >
        <DialogContent className="w-[440px]">
          <DialogHeader>
            <DialogTitle className="text-center">{intl.formatMessage(i18n.heading)}</DialogTitle>
          </DialogHeader>
          <p className="text-text-muted text-sm">
            {intl.formatMessage(i18n.description)}{' '}
            <button
              onClick={(): void => setShowPrivacyInfo(true)}
              className="text-blue-600 dark:text-blue-400 hover:underline"
            >
              {intl.formatMessage(i18n.learnMore)}
            </button>
          </p>
          <DialogFooter className="flex flex-col gap-2 sm:flex-col">
            <Button
              autoFocus
              onClick={(): void => {
                void handleChoice(true);
              }}
              disabled={isSubmitting}
              className="w-full"
            >
              {intl.formatMessage(i18n.optIn)}
            </Button>
            <Button
              variant="ghost"
              onClick={(): void => {
                void handleChoice(false);
              }}
              disabled={isSubmitting}
              className="w-full text-text-secondary hover:text-text-primary"
            >
              {intl.formatMessage(i18n.optOut)}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
      <PrivacyInfoModal isOpen={showPrivacyInfo} onClose={() => setShowPrivacyInfo(false)} />
    </>
  );
}
