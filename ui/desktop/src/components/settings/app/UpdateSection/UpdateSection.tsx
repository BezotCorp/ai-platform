import React, { useState, useEffect } from 'react';
import { Button } from '../../../ui/button';
import { Loader2, Download, CheckCircle, AlertCircle } from 'lucide-react';
import { errorMessage } from '../../../../utils/conversionUtils';
import { defineMessages, useIntl } from '../../../../i18n';
import type { MessageValue, NoMessageValues } from 'react-intl';

import type { UpdateStatus } from '../UpdateSection/updateStatus';
import type { UpdateInfo } from '../UpdateSection/updateInfo';
import type { UpdateEventData } from '../UpdateSection/updateEventData';
const i18n = defineMessages<{
  readonly disableAutoDownload: NoMessageValues;
  readonly disableAutoDownloadDesc: NoMessageValues;
  readonly autoDownloadDisabledByEnv: NoMessageValues;
  readonly downloadNow: NoMessageValues;
  readonly autoDownloadDisabledNote: NoMessageValues;
  readonly loading: NoMessageValues;
  readonly currentVersion: NoMessageValues;
  readonly versionAvailable: { readonly version: MessageValue };
  readonly upToDate: NoMessageValues;
  readonly checkForUpdates: NoMessageValues;
  readonly installAndRestart: NoMessageValues;
  readonly checking: NoMessageValues;
  readonly downloadingProgress: { readonly percent: MessageValue };
  readonly downloadReady: NoMessageValues;
  readonly latestVersion: NoMessageValues;
  readonly updateAvailable: NoMessageValues;
  readonly versionIsAvailable: { readonly version: MessageValue };
  readonly downloadingUpdate: NoMessageValues;
  readonly autoDownload: NoMessageValues;
}>({
  disableAutoDownload: {
    id: 'updateSection.disableAutoDownload',
    defaultMessage: 'Disable automatic update downloads',
  },
  disableAutoDownloadDesc: {
    id: 'updateSection.disableAutoDownloadDesc',
    defaultMessage:
      'When enabled, Goose will notify you of new versions but will not download them automatically.',
  },
  autoDownloadDisabledByEnv: {
    id: 'updateSection.autoDownloadDisabledByEnv',
    defaultMessage:
      'Automatic downloads are disabled via the BCAIP_DISABLE_AUTO_DOWNLOAD environment variable.',
  },
  downloadNow: {
    id: 'updateSection.downloadNow',
    defaultMessage: 'Download Now',
  },
  autoDownloadDisabledNote: {
    id: 'updateSection.autoDownloadDisabledNote',
    defaultMessage: 'Automatic download is disabled. Click "Download Now" to download manually.',
  },
  loading: {
    id: 'updateSection.loading',
    defaultMessage: 'Loading...',
  },
  currentVersion: {
    id: 'updateSection.currentVersion',
    defaultMessage: 'Current version',
  },
  versionAvailable: {
    id: 'updateSection.versionAvailable',
    defaultMessage: '→ {version} available',
  },
  upToDate: {
    id: 'updateSection.upToDate',
    defaultMessage: '(up to date)',
  },
  checkForUpdates: {
    id: 'updateSection.checkForUpdates',
    defaultMessage: 'Check for Updates',
  },
  installAndRestart: {
    id: 'updateSection.installAndRestart',
    defaultMessage: 'Install & Restart',
  },
  checking: {
    id: 'updateSection.checking',
    defaultMessage: 'Checking for updates...',
  },
  downloadingProgress: {
    id: 'updateSection.downloadingProgress',
    defaultMessage: 'Downloading update... {percent}%',
  },
  downloadReady: {
    id: 'updateSection.downloadReady',
    defaultMessage: 'Update downloaded and ready to install!',
  },
  latestVersion: {
    id: 'updateSection.latestVersion',
    defaultMessage: 'You are running the latest version!',
  },
  updateAvailable: {
    id: 'updateSection.updateAvailable',
    defaultMessage: 'Update available!',
  },
  versionIsAvailable: {
    id: 'updateSection.versionIsAvailable',
    defaultMessage: 'Version {version} is available',
  },
  downloadingUpdate: {
    id: 'updateSection.downloadingUpdate',
    defaultMessage: 'Downloading update...',
  },
  autoDownload: {
    id: 'updateSection.autoDownload',
    defaultMessage:
      'Goose will download the update in the background and install it the next time you quit or restart.',
  },
});


export default function UpdateSection(): React.JSX.Element {
  const intl: ReturnType<typeof useIntl> = useIntl();
  const [updateStatus, setUpdateStatus] = useState<UpdateStatus>('idle');
  const [updateInfo, setUpdateInfo] = useState<UpdateInfo>({
    currentVersion: window.electron.getVersion(),
  });
  const [progress, setProgress] = useState<number>(0);
  const [disableAutoDownload, setDisableAutoDownload] = useState<boolean>(false);
  const [autoDownloadForcedByEnv, setAutoDownloadForcedByEnv] = useState<boolean>(false);
  const progressTimeoutRef = React.useRef<ReturnType<typeof setTimeout> | null>(null);
  const lastProgressRef = React.useRef<number>(0);

  useEffect(() => {
    void window.electron.getUpdateState().then((state) => {
      if (state) {
        setUpdateInfo((prev) => ({
          ...prev,
          isUpdateAvailable: state.updateAvailable,
          latestVersion: state.latestVersion,
        }));
      }
    });

    void window.electron.getSetting('disableAutoDownload').then((stored) => {
      setDisableAutoDownload(stored);
    });
    void window.electron.getAutoDownloadDisabled().then((effective) => {
      void window.electron.getSetting('disableAutoDownload').then((stored) => {
        setAutoDownloadForcedByEnv(effective && !stored);
      });
    });

    window.electron.onUpdaterEvent((event) => {
      switch (event.event) {
        case 'checking-for-update':
          setUpdateStatus('checking');
          break;

        case 'update-available':
          setUpdateStatus('idle');
          setUpdateInfo((prev) => ({
            ...prev,
            latestVersion: isUpdateEventData(event.data) ? event.data.version : undefined,
            isUpdateAvailable: true,
          }));
          break;

        case 'update-not-available':
          setUpdateStatus('idle');
          setUpdateInfo((prev) => ({
            ...prev,
            isUpdateAvailable: false,
          }));
          break;

        case 'download-progress': {
          setUpdateStatus('downloading');

          const rawPercent: number | undefined = isUpdateEventData(event.data)
            ? event.data.percent
            : undefined;
          const newProgress: number = typeof rawPercent === 'number' ? Math.round(rawPercent) : 0;

          if (newProgress > lastProgressRef.current) {
            lastProgressRef.current = newProgress;

            if (progressTimeoutRef.current) {
              clearTimeout(progressTimeoutRef.current);
            }

            progressTimeoutRef.current = setTimeout((): void => {
              setProgress(newProgress);
            }, 50);
          }
          break;
        }

        case 'update-downloaded':
          setUpdateStatus('ready');
          setProgress(100);
          break;

        case 'error':
          setUpdateStatus('error');
          setUpdateInfo((prev) => ({
            ...prev,
            error: errorMessage(event.data, 'An error occurred'),
          }));
          setTimeout((): void => setUpdateStatus('idle'), 5000);
          break;
      }
    });

    return () => {
      if (progressTimeoutRef.current) {
        clearTimeout(progressTimeoutRef.current);
      }
    };
  }, []);

  const checkForUpdates = async (): Promise<void> => {
    setUpdateStatus('checking');
    setProgress(0);
    lastProgressRef.current = 0;

    try {
      const result: Awaited<ReturnType<typeof window.electron.checkForUpdates>> =
        await window.electron.checkForUpdates();

      if (result.error) {
        throw new Error(result.error);
      }

      if (!result.error && updateInfo.isUpdateAvailable === false) {
        setUpdateStatus('success');
        setTimeout((): void => setUpdateStatus('idle'), 3000);
      }
    } catch (error) {
      console.error('Error checking for updates:', error);
      setUpdateInfo((prev) => ({
        ...prev,
        error: errorMessage(error, 'Failed to check for updates'),
      }));
      setUpdateStatus('error');
      setTimeout((): void => setUpdateStatus('idle'), 5000);
    }
  };

  const installUpdate = (): void => {
    window.electron.installUpdate();
  };

  const downloadUpdate = async (): Promise<void> => {
    setUpdateStatus('downloading');
    setProgress(0);
    lastProgressRef.current = 0;
    try {
      const result: Awaited<ReturnType<typeof window.electron.downloadUpdate>> =
        await window.electron.downloadUpdate();
      if (result.error) {
        throw new Error(result.error);
      }
    } catch (error) {
      setUpdateInfo((prev) => ({
        ...prev,
        error: errorMessage(error, 'Failed to download update'),
      }));
      setUpdateStatus('error');
      setTimeout((): void => setUpdateStatus('idle'), 5000);
    }
  };

  const toggleAutoDownload = async (disabled: boolean): Promise<void> => {
    setDisableAutoDownload(disabled);
    await window.electron.setSetting('disableAutoDownload', disabled);
  };

  const getStatusMessage = (): string => {
    switch (updateStatus) {
      case 'checking':
        return intl.formatMessage(i18n.checking);
      case 'downloading':
        return intl.formatMessage(i18n.downloadingProgress, { percent: Math.round(progress) });
      case 'ready':
        return intl.formatMessage(i18n.downloadReady);
      case 'success':
        return updateInfo.isUpdateAvailable === false
          ? intl.formatMessage(i18n.latestVersion)
          : intl.formatMessage(i18n.updateAvailable);
      case 'error':
        return updateInfo.error || 'An error occurred';
      default:
        if (updateInfo.isUpdateAvailable) {
          return intl.formatMessage(i18n.versionIsAvailable, { version: updateInfo.latestVersion });
        }
        return '';
    }
  };

  const getStatusIcon = (): React.JSX.Element | null => {
    switch (updateStatus) {
      case 'checking':
      case 'downloading':
        return <Loader2 className="w-4 h-4 animate-spin" />;
      case 'success':
        return <CheckCircle className="w-4 h-4 text-green-500" />;
      case 'error':
        return <AlertCircle className="w-4 h-4 text-red-500" />;
      case 'ready':
        return <CheckCircle className="w-4 h-4 text-blue-500" />;
      default:
        return updateInfo.isUpdateAvailable ? <Download className="w-4 h-4" /> : null;
    }
  };

  const autoDownloadEffectivelyDisabled: boolean = disableAutoDownload || autoDownloadForcedByEnv;

  return (
    <div>
      <div className="text-sm text-text-secondary mb-4 flex items-center gap-2">
        <div className="flex flex-col">
          <div className="text-text-primary text-2xl font-mono">
            {updateInfo.currentVersion || intl.formatMessage(i18n.loading)}
          </div>
          <div className="text-xs text-text-secondary">
            {intl.formatMessage(i18n.currentVersion)}
          </div>
        </div>
        {updateInfo.latestVersion && updateInfo.isUpdateAvailable && (
          <span className="text-text-secondary">
            {' '}
            {intl.formatMessage(i18n.versionAvailable, { version: updateInfo.latestVersion })}
          </span>
        )}
        {updateInfo.currentVersion && updateInfo.isUpdateAvailable === false && (
          <span className="text-text-primary"> {intl.formatMessage(i18n.upToDate)}</span>
        )}
      </div>

      <div className="flex gap-2">
        <div className="flex items-center gap-2">
          <Button
            onClick={(): void => {
              void checkForUpdates();
            }}
            disabled={updateStatus !== 'idle' && updateStatus !== 'error'}
            variant="secondary"
            size="sm"
          >
            {intl.formatMessage(i18n.checkForUpdates)}
          </Button>

          {updateInfo.isUpdateAvailable &&
            updateStatus === 'idle' &&
            autoDownloadEffectivelyDisabled && (
              <Button
                onClick={(): void => {
                  void downloadUpdate();
                }}
                variant="secondary"
                size="sm"
              >
                {intl.formatMessage(i18n.downloadNow)}
              </Button>
            )}

          {updateStatus === 'ready' && (
            <Button onClick={installUpdate} variant="default" size="sm">
              {intl.formatMessage(i18n.installAndRestart)}
            </Button>
          )}
        </div>

        {getStatusMessage() && (
          <div className="flex items-center gap-2 text-xs text-text-secondary">
            {getStatusIcon()}
            <span>{getStatusMessage()}</span>
          </div>
        )}

        {updateStatus === 'downloading' && (
          <div className="w-full mt-2">
            <div className="flex justify-between text-xs text-text-secondary mb-1">
              <span>{intl.formatMessage(i18n.downloadingUpdate)}</span>
              <span>{progress}%</span>
            </div>
            <div className="w-full bg-gray-200 dark:bg-gray-700 rounded-full h-2 overflow-hidden">
              <div
                className="bg-blue-500 h-2 rounded-full transition-[width] duration-150 ease-out"
                style={{ width: `${Math.max(progress, 0)}%`, minWidth: progress > 0 ? '8px' : '0' }}
              />
            </div>
          </div>
        )}

        {updateInfo.isUpdateAvailable && updateStatus === 'idle' && (
          <div className="text-xs text-text-secondary mt-4 space-y-1">
            {autoDownloadEffectivelyDisabled ? (
              <p className="text-xs text-amber-600">
                {intl.formatMessage(i18n.autoDownloadDisabledNote)}
              </p>
            ) : (
              <p>{intl.formatMessage(i18n.autoDownload)}</p>
            )}
          </div>
        )}
      </div>

      <div className="mt-6 pt-4 border-t border-borderSubtle">
        {autoDownloadForcedByEnv ? (
          <p className="text-xs text-amber-600">
            {intl.formatMessage(i18n.autoDownloadDisabledByEnv)}
          </p>
        ) : (
          <label className="flex items-start gap-3 cursor-pointer group">
            <input
              type="checkbox"
              className="mt-0.5 cursor-pointer accent-bgApp"
              checked={disableAutoDownload}
              onChange={(event: React.ChangeEvent<HTMLInputElement>): void => {
                void toggleAutoDownload(event.target.checked);
              }}
            />
            <div>
              <p className="text-sm text-text-primary group-hover:text-text-primary">
                {intl.formatMessage(i18n.disableAutoDownload)}
              </p>
              <p className="text-xs text-text-secondary mt-0.5">
                {intl.formatMessage(i18n.disableAutoDownloadDesc)}
              </p>
            </div>
          </label>
        )}
      </div>
    </div>
  );
}

function isUpdateEventData(value: unknown): value is UpdateEventData {
  return typeof value === 'object' && value !== null;
}
