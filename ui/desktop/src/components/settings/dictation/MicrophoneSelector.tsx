import { useState, useEffect, useRef, useCallback } from 'react';
import type { JSX } from 'react';
import { ChevronDown, Mic } from 'lucide-react';
import { Button } from '../../ui/button';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuTrigger,
} from '../../ui/dropdown-menu';
import { defineMessages, useIntl } from '../../../i18n';
import type { MessageValue, NoMessageValues } from 'react-intl';

const i18n = defineMessages<{
  readonly microphone: NoMessageValues;
  readonly grantAccessDescription: NoMessageValues;
  readonly grantAccess: NoMessageValues;
  readonly chooseDescription: NoMessageValues;
  readonly systemDefault: NoMessageValues;
  readonly selectedMicrophone: NoMessageValues;
  readonly microphoneLabel: { readonly index: MessageValue };
  readonly stop: NoMessageValues;
  readonly test: NoMessageValues;
  readonly speakToTest: { readonly seconds: MessageValue };
}>({
  microphone: {
    id: 'microphoneSelector.microphone',
    defaultMessage: 'Microphone',
  },
  grantAccessDescription: {
    id: 'microphoneSelector.grantAccessDescription',
    defaultMessage: 'Grant access to see available microphones',
  },
  grantAccess: {
    id: 'microphoneSelector.grantAccess',
    defaultMessage: 'Grant Access',
  },
  chooseDescription: {
    id: 'microphoneSelector.chooseDescription',
    defaultMessage: 'Choose which microphone to use for dictation',
  },
  systemDefault: {
    id: 'microphoneSelector.systemDefault',
    defaultMessage: 'System Default',
  },
  selectedMicrophone: {
    id: 'microphoneSelector.selectedMicrophone',
    defaultMessage: 'Selected Microphone',
  },
  microphoneLabel: {
    id: 'microphoneSelector.microphoneLabel',
    defaultMessage: 'Microphone {index}',
  },
  stop: {
    id: 'microphoneSelector.stop',
    defaultMessage: 'Stop',
  },
  test: {
    id: 'microphoneSelector.test',
    defaultMessage: 'Test',
  },
  speakToTest: {
    id: 'microphoneSelector.speakToTest',
    defaultMessage: 'Speak to test your microphone ({seconds}s)',
  },
});

interface MicrophoneSelectorProps {
  selectedDeviceId: string | null;
  onDeviceChange: (deviceId: string | null) => void;
}

const TEST_DURATION_MS = 5000;

export const MicrophoneSelector = ({
  selectedDeviceId,
  onDeviceChange,
}: MicrophoneSelectorProps): JSX.Element => {
  const intl: ReturnType<typeof useIntl> = useIntl();
  const [devices, setDevices] = useState<MediaDeviceInfo[]>([]);
  const [hasPermission, setHasPermission] = useState<boolean>(false);
  const [isTesting, setIsTesting] = useState<boolean>(false);
  const [vuLevel, setVuLevel] = useState<number>(0);

  const testStreamRef = useRef<MediaStream | null>(null);
  const testCtxRef = useRef<AudioContext | null>(null);
  const rafRef = useRef<number>(0);
  const testTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const testGenerationRef = useRef<number>(0);

  const enumerate = useCallback(async (): Promise<void> => {
    try {
      const all: MediaDeviceInfo[] = await navigator.mediaDevices.enumerateDevices();
      const inputs: MediaDeviceInfo[] = all.filter(
        (device: MediaDeviceInfo): boolean => device.kind === 'audioinput'
      );
      setHasPermission(inputs.some((device: MediaDeviceInfo): boolean => device.label !== ''));
      setDevices(inputs);
    } catch (e) {
      console.error('Failed to enumerate devices:', e);
    }
  }, []);

  useEffect(() => {
    queueMicrotask((): void => {
      void enumerate();
    });
    navigator.mediaDevices.addEventListener('devicechange', enumerate);
    return () => navigator.mediaDevices.removeEventListener('devicechange', enumerate);
  }, [enumerate]);

  const requestPermission = async (): Promise<void> => {
    try {
      const stream: MediaStream = await navigator.mediaDevices.getUserMedia({ audio: true });
      stream.getTracks().forEach((track: MediaStreamTrack): void => track.stop());
      await enumerate();
    } catch (e) {
      console.error('Microphone permission denied:', e);
    }
  };

  const stopTest = useCallback((): void => {
    testGenerationRef.current += 1;
    if (rafRef.current) cancelAnimationFrame(rafRef.current);
    rafRef.current = 0;
    if (testTimerRef.current) clearTimeout(testTimerRef.current);
    testTimerRef.current = null;
    void testCtxRef.current?.close();
    testCtxRef.current = null;
    testStreamRef.current?.getTracks().forEach((track: MediaStreamTrack): void => track.stop());
    testStreamRef.current = null;
    setIsTesting(false);
    setVuLevel(0);
  }, []);

  const startTest = async (): Promise<void> => {
    stopTest();
    const generation: number = testGenerationRef.current;
    setIsTesting(true);

    try {
      const constraints: MediaTrackConstraints = {
        echoCancellation: true,
        noiseSuppression: true,
        autoGainControl: true,
      };
      if (selectedDeviceId) {
        constraints.deviceId = { exact: selectedDeviceId };
      }

      const stream: MediaStream = await navigator.mediaDevices.getUserMedia({ audio: constraints });
      if (testGenerationRef.current !== generation) {
        stream.getTracks().forEach((track: MediaStreamTrack): void => track.stop());
        return;
      }
      testStreamRef.current = stream;

      const ctx: AudioContext = new AudioContext();
      testCtxRef.current = ctx;
      const source: MediaStreamAudioSourceNode = ctx.createMediaStreamSource(stream);
      const analyser: AnalyserNode = ctx.createAnalyser();
      analyser.fftSize = 256;
      source.connect(analyser);

      const dataArray = new Uint8Array(analyser.frequencyBinCount);

      const poll = (): void => {
        if (testGenerationRef.current !== generation) return;

        analyser.getByteTimeDomainData(dataArray);
        let sum: number = 0;
        for (let i: number = 0; i < dataArray.length; i += 1) {
          const v: number = (dataArray[i] - 128) / 128;
          sum += v * v;
        }
        const rms: number = Math.sqrt(sum / dataArray.length);
        setVuLevel(Math.min(1, rms * 5));
        rafRef.current = requestAnimationFrame(poll);
      };

      rafRef.current = requestAnimationFrame(poll);
      testTimerRef.current = setTimeout((): void => {
        if (testGenerationRef.current === generation) stopTest();
      }, TEST_DURATION_MS);
    } catch (e) {
      if (testGenerationRef.current !== generation) return;

      console.error('Mic test failed:', e);
      stopTest();
    }
  };

  useEffect(() => {
    return () => stopTest();
  }, [stopTest]);

  const getDeviceLabel = (device: MediaDeviceInfo, index: number): string => {
    return device.label || intl.formatMessage(i18n.microphoneLabel, { index: index + 1 });
  };

  const selectedLabel = (): string => {
    if (!selectedDeviceId) return intl.formatMessage(i18n.systemDefault);
    const device: MediaDeviceInfo | undefined = devices.find(
      (mediaDevice: MediaDeviceInfo): boolean => mediaDevice.deviceId === selectedDeviceId
    );
    if (device) return device.label || intl.formatMessage(i18n.selectedMicrophone);
    return intl.formatMessage(i18n.systemDefault);
  };

  if (!hasPermission) {
    return (
      <div className="flex items-center justify-between py-2 px-2 hover:bg-background-secondary rounded-lg transition-all">
        <div>
          <h3 className="text-text-primary text-sm">{intl.formatMessage(i18n.microphone)}</h3>
          <p className="text-xs text-text-secondary max-w-md mt-[2px]">
            {intl.formatMessage(i18n.grantAccessDescription)}
          </p>
        </div>
        <Button
          variant="outline"
          size="sm"
          onClick={(): void => {
            void requestPermission();
          }}
        >
          {intl.formatMessage(i18n.grantAccess)}
        </Button>
      </div>
    );
  }

  return (
    <div className="space-y-3">
      <div className="flex items-center justify-between py-2 px-2 hover:bg-background-secondary rounded-lg transition-all">
        <div>
          <h3 className="text-text-primary text-sm">{intl.formatMessage(i18n.microphone)}</h3>
          <p className="text-xs text-text-secondary max-w-md mt-[2px]">
            {intl.formatMessage(i18n.chooseDescription)}
          </p>
        </div>
        <div className="flex items-center gap-2">
          <DropdownMenu>
            <DropdownMenuTrigger className="flex items-center gap-2 px-3 py-1.5 text-sm border border-border-primary rounded-md hover:border-border-primary transition-colors text-text-primary bg-background-primary max-w-[220px]">
              <span className="truncate">{selectedLabel()}</span>
              <ChevronDown className="w-4 h-4 shrink-0" />
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end" className="w-max min-w-[250px] max-w-[350px]">
              <DropdownMenuRadioGroup
                value={selectedDeviceId ?? 'system_default'}
                onValueChange={(value: string): void =>
                  onDeviceChange(value === 'system_default' ? null : value)
                }
              >
                <DropdownMenuRadioItem value="system_default">
                  {intl.formatMessage(i18n.systemDefault)}
                </DropdownMenuRadioItem>
                {devices.map((device: MediaDeviceInfo, index: number) => (
                  <DropdownMenuRadioItem key={device.deviceId} value={device.deviceId}>
                    <span className="truncate">{getDeviceLabel(device, index)}</span>
                  </DropdownMenuRadioItem>
                ))}
              </DropdownMenuRadioGroup>
            </DropdownMenuContent>
          </DropdownMenu>
          <Button
            variant="outline"
            size="sm"
            onClick={(): void => {
              if (isTesting) {
                stopTest();
                return;
              }

              void startTest();
            }}
            className="shrink-0"
          >
            <Mic className="w-4 h-4 mr-1" />
            {isTesting ? intl.formatMessage(i18n.stop) : intl.formatMessage(i18n.test)}
          </Button>
        </div>
      </div>

      {isTesting && (
        <div className="px-2">
          <div className="w-full bg-background-secondary rounded-full h-2 overflow-hidden">
            <div
              className="bg-green-500 h-2 rounded-full transition-all duration-75"
              style={{ width: `${vuLevel * 100}%` }}
            />
          </div>
          <p className="text-xs text-text-secondary mt-1">
            {intl.formatMessage(i18n.speakToTest, { seconds: Math.ceil(TEST_DURATION_MS / 1000) })}
          </p>
        </div>
      )}
    </div>
  );
};
