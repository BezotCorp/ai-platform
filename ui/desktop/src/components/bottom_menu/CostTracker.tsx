import { useState, useEffect } from 'react';
import type { JSX } from 'react';
import { Tooltip, TooltipContent, TooltipTrigger } from '../ui/Tooltip';
import { fetchCanonicalModelInfo, type CanonicalModelInfo } from '../../utils/canonicalModelInfo';
import { defineMessages, useIntl } from '../../i18n';
import type { MessageValue } from 'react-intl';

const i18n = defineMessages<{
  readonly pricingUnavailable: { readonly model: MessageValue };
  readonly costUnavailable: {
    readonly inputTokens: MessageValue;
    readonly model: MessageValue;
    readonly outputTokens: MessageValue;
  };
  readonly totalSessionCost: { readonly cost: MessageValue };
  readonly inputOutputTooltip: {
    readonly inputCost: MessageValue;
    readonly inputTokens: MessageValue;
    readonly outputCost: MessageValue;
    readonly outputTokens: MessageValue;
  };
}>({
  pricingUnavailable: {
    id: 'costTracker.pricingUnavailable',
    defaultMessage: 'Pricing data unavailable for {model}',
  },
  costUnavailable: {
    id: 'costTracker.costUnavailable',
    defaultMessage:
      'Cost data not available for {model} ({inputTokens} input, {outputTokens} output tokens)',
  },
  totalSessionCost: {
    id: 'costTracker.totalSessionCost',
    defaultMessage: 'Total session cost: {cost}',
  },
  inputOutputTooltip: {
    id: 'costTracker.inputOutputTooltip',
    defaultMessage:
      'Input: {inputTokens} tokens ({inputCost}) | Output: {outputTokens} tokens ({outputCost})',
  },
});

interface CostTrackerProps {
  inputTokens?: number;
  outputTokens?: number;
  accumulatedCost?: number | null;
  model: string | null;
  provider: string | null;
}

export function CostTracker({
  inputTokens = 0,
  outputTokens = 0,
  accumulatedCost,
  model: currentModel,
  provider: currentProvider,
}: CostTrackerProps): JSX.Element | null {
  const intl: ReturnType<typeof useIntl> = useIntl();
  const [costInfo, setCostInfo] = useState<CanonicalModelInfo | null>(null);
  const [isLoading, setIsLoading] = useState<boolean>(true);
  const [showPricing, setShowPricing] = useState<boolean>(true);
  const [pricingFailed, setPricingFailed] = useState<boolean>(false);

  // Check if pricing is enabled
  useEffect(() => {
    const loadPricingSetting = async (): Promise<void> => {
      const enabled: boolean = await window.electron.getSetting('showPricing');
      setShowPricing(enabled);
    };

    void loadPricingSetting();

    const handlePricingChange = (): void => {
      void loadPricingSetting();
    };

    window.addEventListener('showPricingChanged', handlePricingChange);
    return () => window.removeEventListener('showPricingChanged', handlePricingChange);
  }, []);

  useEffect(() => {
    const loadCostInfo = async (): Promise<void> => {
      if (!currentModel || !currentProvider) {
        setIsLoading(false);
        return;
      }

      setIsLoading(true);
      try {
        const costData: CanonicalModelInfo | null = await fetchCanonicalModelInfo(
          currentProvider,
          currentModel
        );
        if (costData) {
          setCostInfo(costData);
          setPricingFailed(false);
        } else {
          setPricingFailed(true);
          setCostInfo(null);
        }
      } catch {
        setPricingFailed(true);
        setCostInfo(null);
      } finally {
        setIsLoading(false);
      }
    };

    void loadCostInfo();
  }, [currentModel, currentProvider]);

  // Return null early if pricing is disabled
  if (!showPricing) {
    return null;
  }

  const calculateCost = (): number => {
    return accumulatedCost ?? 0;
  };

  const formatCost = (cost: number): string => cost.toFixed(2);

  // Show loading state or when we don't have model/provider info
  if (!currentModel || !currentProvider) {
    return null;
  }

  // If still loading, show a placeholder
  if (isLoading) {
    return (
      <div className="flex items-center justify-center h-full text-text-secondary translate-y-[1px]">
        <span className="text-xs font-mono">...</span>
      </div>
    );
  }

  const currency: string = costInfo?.currency || '$';

  if (
    accumulatedCost == null &&
    (!costInfo || (costInfo.inputTokenCost === undefined && costInfo.outputTokenCost === undefined))
  ) {
    const freeProviders: string[] = ['ollama', 'local', 'localhost'];
    if (freeProviders.includes(currentProvider.toLowerCase())) {
      return (
        <div className="flex items-center justify-center h-full text-text-primary/70 transition-colors cursor-default translate-y-[1px]">
          <span className="text-xs font-mono">
            {inputTokens.toLocaleString()}↑ {outputTokens.toLocaleString()}↓
          </span>
        </div>
      );
    }

    // Otherwise show as unavailable
    const getUnavailableTooltip = (): string => {
      if (pricingFailed) {
        return intl.formatMessage(i18n.pricingUnavailable, { model: currentModel });
      }
      return intl.formatMessage(i18n.costUnavailable, {
        model: currentModel,
        inputTokens: inputTokens.toLocaleString(),
        outputTokens: outputTokens.toLocaleString(),
      });
    };

    return (
      <Tooltip>
        <TooltipTrigger asChild>
          <div className="flex items-center justify-center h-full transition-colors cursor-default translate-y-[1px] text-text-primary/70 hover:text-text-primary">
            <span className="text-xs font-mono">
              {currency}
              {formatCost(0)}
            </span>
          </div>
        </TooltipTrigger>
        <TooltipContent>{getUnavailableTooltip()}</TooltipContent>
      </Tooltip>
    );
  }

  const totalCost: number = calculateCost();

  // Build tooltip content
  const getTooltipContent = (): string => {
    if (pricingFailed) {
      return intl.formatMessage(i18n.pricingUnavailable, {
        model: `${currentProvider}/${currentModel}`,
      });
    }

    if (accumulatedCost != null) {
      const totalCostText: string = intl.formatMessage(i18n.totalSessionCost, {
        cost: `${currency}${totalCost.toFixed(4)}`,
      });
      const tokenCostText: string = intl.formatMessage(i18n.inputOutputTooltip, {
        inputTokens: inputTokens.toLocaleString(),
        inputCost: `${currency}${((inputTokens * (costInfo?.inputTokenCost || 0)) / 1_000_000).toFixed(6)}`,
        outputTokens: outputTokens.toLocaleString(),
        outputCost: `${currency}${((outputTokens * (costInfo?.outputTokenCost || 0)) / 1_000_000).toFixed(6)}`,
      });
      return `${totalCostText}\n${tokenCostText}`;
    }

    const inputCostStr: string = `${currency}${((inputTokens * (costInfo?.inputTokenCost || 0)) / 1_000_000).toFixed(6)}`;
    const outputCostStr: string = `${currency}${((outputTokens * (costInfo?.outputTokenCost || 0)) / 1_000_000).toFixed(6)}`;
    return intl.formatMessage(i18n.inputOutputTooltip, {
      inputTokens: inputTokens.toLocaleString(),
      inputCost: inputCostStr,
      outputTokens: outputTokens.toLocaleString(),
      outputCost: outputCostStr,
    });
  };

  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <div className="flex items-center justify-center h-full transition-colors cursor-default translate-y-[1px] text-text-primary/70 hover:text-text-primary">
          <span className="text-xs font-mono">
            {currency}
            {formatCost(totalCost)}
          </span>
        </div>
      </TooltipTrigger>
      <TooltipContent>{getTooltipContent()}</TooltipContent>
    </Tooltip>
  );
}
