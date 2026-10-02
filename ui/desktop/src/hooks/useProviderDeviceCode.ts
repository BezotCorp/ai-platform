import { useEffect, useState } from 'react';
import type { ProviderDeviceCodeNotificationUnstable } from '@aaif/goose-acp-client';

export function useProviderDeviceCode(providerId: string) {
  const [deviceCode, setDeviceCode] = useState<ProviderDeviceCodeNotificationUnstable | null>(
    null
  );

  useEffect(() => {
    const handler = (event: Event) => {
      const detail = (event as CustomEvent<ProviderDeviceCodeNotificationUnstable>).detail;
      if (detail.providerId === providerId) {
        setDeviceCode(detail);
      }
    };
    window.addEventListener('goose:device-code', handler);
    return () => window.removeEventListener('goose:device-code', handler);
  }, [providerId]);

  return {
    deviceCode: deviceCode?.providerId === providerId ? deviceCode : null,
    clearDeviceCode: () => setDeviceCode(null),
  };
}
