import type { ModelsSectionProps } from './modelsSectionProps';

import { useEffect, useState, useCallback, useRef } from 'react';
import type { JSX } from 'react';
import { View } from '../../../utils/navigationUtils';
import ModelSettingsButtons from './subcomponents/ModelSettingsButtons';
import { acpGetProviderDetails, acpReadDefaults } from '../../../acp/providers';
import { modelAndProviderMessages, useModelAndProvider } from '../../ModelAndProviderContext';
import { toastError } from '../../../toastService';

import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '../../ui/card';
import ResetProviderSection from '../reset_provider/ResetProviderSection';
import { defineMessages, useIntl } from '../../../i18n';
import type { NoMessageValues } from 'react-intl';

const i18n = defineMessages<{
  readonly resetTitle: NoMessageValues;
  readonly resetDescription: NoMessageValues;
}>({
  resetTitle: {
    id: 'modelsSection.resetTitle',
    defaultMessage: 'Reset Provider and Model',
  },
  resetDescription: {
    id: 'modelsSection.resetDescription',
    defaultMessage: 'Clear your selected model and provider settings to start fresh',
  },
});


export default function ModelsSection({ setView }: ModelsSectionProps): JSX.Element {
  const intl: ReturnType<typeof useIntl> = useIntl();
  const [provider, setProvider] = useState<string | null>(null);
  const [displayModelName, setDisplayModelName] = useState<string>('');
  const [isLoading, setIsLoading] = useState<boolean>(true);
  const {
    getCurrentModelDisplayName,
    getCurrentProviderDisplayName,
    currentModel,
    currentProvider,
  } = useModelAndProvider();

  const loadModelData = useCallback(async (): Promise<void> => {
    try {
      setIsLoading(true);

      // Get display name (alias if available, otherwise model name)
      const modelDisplayName: string = await getCurrentModelDisplayName();
      setDisplayModelName(modelDisplayName);

      // Get provider display name (subtext if available from predefined models, otherwise provider metadata)
      const providerDisplayName: string | null = await getCurrentProviderDisplayName();
      if (providerDisplayName) {
        setProvider(providerDisplayName);
      } else {
        // Fallback to original provider lookup
        const { providerId: gooseProvider } = await acpReadDefaults();
        if (!gooseProvider) {
          setProvider('');
          return;
        }
        try {
          const providerDetails = await acpGetProviderDetails(gooseProvider);
          setProvider(providerDetails.metadata.display_name);
        } catch {
          toastError({
            title: intl.formatMessage(modelAndProviderMessages.unknownProviderTitle),
            msg: intl.formatMessage(modelAndProviderMessages.unknownProviderMsg),
          });
          setProvider(gooseProvider);
        }
      }
    } catch (error) {
      console.error('Error loading model data:', error);
    } finally {
      setIsLoading(false);
    }
  }, [getCurrentModelDisplayName, getCurrentProviderDisplayName, intl]);

  useEffect(() => {
    queueMicrotask((): void => {
      void loadModelData();
    });
  }, [loadModelData]);

  // Update display when model or provider changes - but only if they actually changed
  const prevModelRef = useRef<string | null>(null);
  const prevProviderRef = useRef<string | null>(null);

  useEffect(() => {
    if (
      currentModel &&
      currentProvider &&
      (currentModel !== prevModelRef.current || currentProvider !== prevProviderRef.current)
    ) {
      prevModelRef.current = currentModel;
      prevProviderRef.current = currentProvider;
      void loadModelData();
    }
  }, [currentModel, currentProvider, loadModelData]);

  return (
    <section id="models" className="space-y-4 pr-4">
      <Card className="p-2 pb-4">
        <CardContent className="px-2">
          {isLoading ? (
            <>
              <div className="h-[20px] mb-1"></div>
              <div className="h-[16px]"></div>
            </>
          ) : (
            <div className="animate-in fade-in duration-100">
              <h3 className="text-text-primary">{displayModelName}</h3>
              <h4 className="text-xs text-text-secondary">{provider}</h4>
            </div>
          )}
          <ModelSettingsButtons setView={setView} />
        </CardContent>
      </Card>
      <Card className="pb-2 rounded-lg">
        <CardHeader className="pb-0">
          <CardTitle className="">{intl.formatMessage(i18n.resetTitle)}</CardTitle>
          <CardDescription>{intl.formatMessage(i18n.resetDescription)}</CardDescription>
        </CardHeader>
        <CardContent className="px-2">
          <ResetProviderSection setView={setView} />
        </CardContent>
      </Card>
    </section>
  );
}
