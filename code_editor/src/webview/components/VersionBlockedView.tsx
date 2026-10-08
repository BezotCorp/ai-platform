/**
 * Component for displaying version-related blocking messages.
 * Shows guidance for installing or updating BCAIP when version requirements are not met.
 */

import { createOpenExternalLinkMessage } from '../../shared/messages';
import { postMessage } from '../bridge';
import { BCAIP_ICON_URL } from './icons/BcaipWatermark';

export interface VersionBlockedViewProps {
  status: 'blocked_missing' | 'blocked_outdated';
  detectedVersion?: string;
  minimumVersion: string;
  installUrl?: string;
  updateUrl?: string;
  /** Set when `bcaip.binaryPath` is configured but invalid */
  configuredPath?: string;
}

export function VersionBlockedView({
  status,
  detectedVersion,
  minimumVersion,
  installUrl,
  updateUrl,
  configuredPath,
}: VersionBlockedViewProps) {
  const handleLinkClick = (url: string) => {
    postMessage(createOpenExternalLinkMessage(url));
  };

  if (status === 'blocked_missing' && configuredPath) {
    return (
      <div className="flex flex-col items-center justify-center h-screen p-6">
        <div className="max-w-md text-center">
          <WarningIcon className="w-12 h-12 mx-auto mb-4 text-[var(--vscode-editorWarning-foreground)]" />
          <h2 className="text-lg font-medium text-[var(--vscode-foreground)] mb-3">
            BCAIP Binary Path Invalid
          </h2>
          <p className="text-sm text-[var(--vscode-descriptionForeground)] mb-2">
            The configured path{' '}
            <code className="font-mono text-[var(--vscode-textPreformat-foreground)] break-all">
              {configuredPath}
            </code>{' '}
            does not exist or is not executable.
          </p>
          <p className="text-sm text-[var(--vscode-descriptionForeground)]">
            Fix the{' '}
            <code className="font-mono text-[var(--vscode-textPreformat-foreground)]">
              bcaip.binaryPath
            </code>{' '}
            setting, or clear it to use auto-detection, then run the "BCAIP: Restart" command to
            search again.
          </p>
        </div>
      </div>
    );
  }

  if (status === 'blocked_missing') {
    return (
      <div className="flex flex-col items-center justify-center h-screen p-6">
        <div className="max-w-md text-center">
          <img
            src={BCAIP_ICON_URL}
            alt=""
            aria-hidden="true"
            className="w-12 h-12 mx-auto mb-4 opacity-60"
          />
          <h2 className="text-lg font-medium text-[var(--vscode-foreground)] mb-3">
            Welcome to BCAIP
          </h2>
          <p className="text-sm text-[var(--vscode-descriptionForeground)] mb-4">
            To get started, you need to install BCAIP (version {minimumVersion} or higher) on your
            system.
          </p>
          {installUrl && (
            <button
              type="button"
              onClick={() => handleLinkClick(installUrl)}
              className="inline-flex items-center gap-2 px-4 py-2 text-sm text-[var(--vscode-button-foreground)] bg-[var(--vscode-button-background)] hover:bg-[var(--vscode-button-hoverBackground)] rounded transition-colors"
            >
              <ExternalLinkIcon className="w-4 h-4" />
              View Installation Guide
            </button>
          )}
        </div>
      </div>
    );
  }

  return (
    <div className="flex flex-col items-center justify-center h-screen p-6">
      <div className="max-w-md text-center">
        <UpdateIcon className="w-12 h-12 mx-auto mb-4 text-[var(--vscode-editorWarning-foreground)]" />
        <h2 className="text-lg font-medium text-[var(--vscode-foreground)] mb-3">
          BCAIP Update Required
        </h2>
        <p className="text-sm text-[var(--vscode-descriptionForeground)] mb-2">
          This extension requires BCAIP version {minimumVersion} or higher.
        </p>
        {detectedVersion && (
          <p className="text-sm text-[var(--vscode-descriptionForeground)] mb-4">
            Your current version:{' '}
            <span className="font-medium text-[var(--vscode-foreground)]">{detectedVersion}</span>
          </p>
        )}
        {updateUrl && (
          <button
            type="button"
            onClick={() => handleLinkClick(updateUrl)}
            className="inline-flex items-center gap-2 px-4 py-2 text-sm text-[var(--vscode-button-foreground)] bg-[var(--vscode-button-background)] hover:bg-[var(--vscode-button-hoverBackground)] rounded transition-colors"
          >
            <ExternalLinkIcon className="w-4 h-4" />
            View Update Instructions
          </button>
        )}
      </div>
    </div>
  );
}

function WarningIcon({ className }: { className?: string }) {
  return (
    <svg
      className={className}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.5"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <path d="M10.29 3.86L1.82 18a2 2 0 001.71 3h16.94a2 2 0 001.71-3L13.71 3.86a2 2 0 00-3.42 0z" />
      <line x1="12" y1="9" x2="12" y2="13" />
      <line x1="12" y1="17" x2="12.01" y2="17" />
    </svg>
  );
}

function UpdateIcon({ className }: { className?: string }) {
  return (
    <svg
      className={className}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.5"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <path d="M21 12a9 9 0 11-6.219-8.56" />
      <polyline points="21 3 21 9 15 9" />
    </svg>
  );
}

function ExternalLinkIcon({ className }: { className?: string }) {
  return (
    <svg
      className={className}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <path d="M18 13v6a2 2 0 01-2 2H5a2 2 0 01-2-2V8a2 2 0 012-2h6" />
      <polyline points="15 3 21 3 21 9" />
      <line x1="10" y1="14" x2="21" y2="3" />
    </svg>
  );
}
