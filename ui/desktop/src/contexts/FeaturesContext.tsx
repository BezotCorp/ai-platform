import { createContext, useContext, useEffect, useState, useMemo } from 'react';
import type { ReactNode } from 'react';
import { getAcpFeatureCapabilities } from '../acp/capabilities';

interface FeaturesContextValue {
  localInference: boolean;
  isLoading: boolean;
}

const FeaturesContext = createContext<FeaturesContextValue | null>(null);

export function FeaturesProvider({ children }: { children: ReactNode }): React.JSX.Element {
  const [localInference, setLocalInference] = useState<boolean>(false);
  const [isLoading, setIsLoading] = useState<boolean>(true);

  useEffect(() => {
    void (async (): Promise<void> => {
      try {
        const capabilities: Awaited<ReturnType<typeof getAcpFeatureCapabilities>> =
          await getAcpFeatureCapabilities();
        setLocalInference(capabilities.localInference);
      } catch (error) {
        console.warn('[FeaturesContext] Failed to fetch features:', error);
      } finally {
        setIsLoading(false);
      }
    })();
  }, []);

  const value = useMemo<FeaturesContextValue>(
    () => ({
      localInference,
      isLoading,
    }),
    [localInference, isLoading]
  );

  return <FeaturesContext.Provider value={value}>{children}</FeaturesContext.Provider>;
}

export function useFeatures(): FeaturesContextValue {
  const context = useContext(FeaturesContext);
  if (!context) {
    throw new Error('useFeatures must be used within a FeaturesProvider');
  }
  return context;
}
