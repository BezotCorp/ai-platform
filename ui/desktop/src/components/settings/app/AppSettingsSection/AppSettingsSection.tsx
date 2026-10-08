import { useState, useEffect, useRef } from 'react';
import type { JSX } from 'react';
import { defineMessages, useIntl } from '../../../../i18n';
import { Switch } from '../../../ui/switch';
import { Button } from '../../../ui/button';
import { ChevronDown, Settings } from 'lucide-react';
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle } from '../../../ui/dialog';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuTrigger,
} from '../../../ui/dropdown-menu';
import UpdateSection from '../UpdateSection';

import { COST_TRACKING_ENABLED, UPDATES_ENABLED } from '../../../../updates';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '../../../ui/card';
import ThemeSelector from '../../../GooseSidebar/ThemeSelector';
import BlockLogoBlack from '../icons/block-lockup_black.png';
import BlockLogoWhite from '../icons/block-lockup_white.png';
import TelemetrySettings from '../TelemetrySettings';
import { trackSettingToggled } from '../../../../utils/analyticsEvent';
import type { MessageValue, NoMessageValues } from 'react-intl';
import { LanguageSetting } from '../../../../utils/languageSetting';
import type { AppSettingsSectionProps } from '../AppSettingsSection/appSettingsSectionProps';

const i18n = defineMessages<{
  readonly appearanceTitle: NoMessageValues;
  readonly appearanceDesc: NoMessageValues;
  readonly notifications: NoMessageValues;
  readonly notificationsDesc: { readonly link: MessageValue };
  readonly configGuide: NoMessageValues;
  readonly openSettings: NoMessageValues;
  readonly taskNotifications: NoMessageValues;
  readonly taskNotificationsDesc: NoMessageValues;
  readonly menuBarIcon: NoMessageValues;
  readonly menuBarIconDesc: NoMessageValues;
  readonly dockIcon: NoMessageValues;
  readonly dockIconDesc: NoMessageValues;
  readonly preventSleep: NoMessageValues;
  readonly preventSleepDesc: NoMessageValues;
  readonly costTracking: NoMessageValues;
  readonly costTrackingDesc: NoMessageValues;
  readonly themeTitle: NoMessageValues;
  readonly themeDesc: NoMessageValues;
  readonly languageTitle: NoMessageValues;
  readonly languageDesc: NoMessageValues;
  readonly languageSystem: NoMessageValues;
  readonly languageEnglish: NoMessageValues;
  readonly languageChineseSimplified: NoMessageValues;
  readonly languageRussian: NoMessageValues;
  readonly languageTurkish: NoMessageValues;
  readonly languageHindi: NoMessageValues;
  readonly languageJapanese: NoMessageValues;
  readonly languageSpanish: NoMessageValues;
  readonly languageKorean: NoMessageValues;
  readonly languageFrench: NoMessageValues;
  readonly languageGerman: NoMessageValues;
  readonly languageItalian: NoMessageValues;
  readonly languagePortuguese: NoMessageValues;
  readonly languageIndonesian: NoMessageValues;
  readonly languageMalay: NoMessageValues;
  readonly languageVietnamese: NoMessageValues;
  readonly languageChineseTraditional: NoMessageValues;
  readonly helpTitle: NoMessageValues;
  readonly helpDesc: NoMessageValues;
  readonly reportBug: NoMessageValues;
  readonly requestFeature: NoMessageValues;
  readonly versionTitle: NoMessageValues;
  readonly updatesTitle: NoMessageValues;
  readonly updatesDesc: NoMessageValues;
  readonly notificationsModalTitle: NoMessageValues;
  readonly notificationsMacInstructions: NoMessageValues;
  readonly notificationsMacStep1: NoMessageValues;
  readonly notificationsMacStep2: NoMessageValues;
  readonly notificationsMacStep3: NoMessageValues;
  readonly notificationsMacStep4: NoMessageValues;
  readonly notificationsWinInstructions: NoMessageValues;
  readonly notificationsWinStep1: NoMessageValues;
  readonly notificationsWinStep2: NoMessageValues;
  readonly notificationsWinStep3: NoMessageValues;
  readonly notificationsWinStep4: NoMessageValues;
  readonly close: NoMessageValues;
}>({
  appearanceTitle: { id: 'settings.appearance.title', defaultMessage: 'Appearance' },
  appearanceDesc: {
    id: 'settings.appearance.description',
    defaultMessage: 'Configure how goose appears on your system',
  },
  notifications: { id: 'settings.notifications.title', defaultMessage: 'Notifications' },
  notificationsDesc: {
    id: 'settings.notifications.description',
    defaultMessage: 'Notifications are managed by your OS - {link}',
  },
  configGuide: { id: 'settings.notifications.configGuide', defaultMessage: 'Configuration guide' },
  openSettings: { id: 'settings.notifications.openSettings', defaultMessage: 'Open Settings' },
  taskNotifications: {
    id: 'settings.notifications.task.title',
    defaultMessage: 'Task completion notifications',
  },
  taskNotificationsDesc: {
    id: 'settings.notifications.task.description',
    defaultMessage: 'Notify when Goose finishes a task while the window is in the background',
  },
  menuBarIcon: { id: 'settings.menuBarIcon.title', defaultMessage: 'Menu bar icon' },
  menuBarIconDesc: {
    id: 'settings.menuBarIcon.description',
    defaultMessage: 'Show goose in the menu bar',
  },
  dockIcon: { id: 'settings.dockIcon.title', defaultMessage: 'Dock icon' },
  dockIconDesc: { id: 'settings.dockIcon.description', defaultMessage: 'Show goose in the dock' },
  preventSleep: { id: 'settings.preventSleep.title', defaultMessage: 'Prevent Sleep' },
  preventSleepDesc: {
    id: 'settings.preventSleep.description',
    defaultMessage:
      'Keep your computer awake while goose is running a task (screen can still lock)',
  },
  costTracking: { id: 'settings.costTracking.title', defaultMessage: 'Cost Tracking' },
  costTrackingDesc: {
    id: 'settings.costTracking.description',
    defaultMessage: 'Show model pricing and usage costs',
  },
  themeTitle: { id: 'settings.theme.title', defaultMessage: 'Theme' },
  themeDesc: {
    id: 'settings.theme.description',
    defaultMessage: 'Customize the look and feel of goose',
  },
  languageTitle: { id: 'settings.language.title', defaultMessage: 'Language' },
  languageDesc: {
    id: 'settings.language.description',
    defaultMessage: 'Choose the display language for goose',
  },
  languageSystem: { id: 'settings.language.systemDefault', defaultMessage: 'System Default' },
  languageEnglish: { id: 'settings.language.english', defaultMessage: 'English' },
  languageChineseSimplified: {
    id: 'settings.language.zhCN',
    defaultMessage: 'Chinese (Simplified)',
  },
  languageRussian: { id: 'settings.language.russian', defaultMessage: 'Russian' },
  languageTurkish: { id: 'settings.language.turkish', defaultMessage: 'Turkish' },
  languageHindi: { id: 'settings.language.hindi', defaultMessage: 'Hindi' },
  languageJapanese: { id: 'settings.language.japanese', defaultMessage: 'Japanese' },
  languageSpanish: { id: 'settings.language.spanish', defaultMessage: 'Spanish' },
  languageKorean: { id: 'settings.language.korean', defaultMessage: 'Korean' },
  languageFrench: { id: 'settings.language.french', defaultMessage: 'French' },
  languageGerman: { id: 'settings.language.german', defaultMessage: 'German' },
  languageItalian: { id: 'settings.language.italian', defaultMessage: 'Italian' },
  languagePortuguese: { id: 'settings.language.portuguese', defaultMessage: 'Portuguese' },
  languageIndonesian: { id: 'settings.language.indonesian', defaultMessage: 'Indonesian' },
  languageMalay: { id: 'settings.language.malay', defaultMessage: 'Malay' },
  languageVietnamese: { id: 'settings.language.vietnamese', defaultMessage: 'Vietnamese' },
  languageChineseTraditional: {
    id: 'settings.language.zhTW',
    defaultMessage: 'Chinese (Traditional)',
  },
  helpTitle: { id: 'settings.help.title', defaultMessage: 'Help & feedback' },
  helpDesc: {
    id: 'settings.help.description',
    defaultMessage: 'Help us improve goose by reporting issues or requesting new features',
  },
  reportBug: { id: 'settings.help.reportBug', defaultMessage: 'Report a Bug' },
  requestFeature: { id: 'settings.help.requestFeature', defaultMessage: 'Request a Feature' },
  versionTitle: { id: 'settings.version.title', defaultMessage: 'Version' },
  updatesTitle: { id: 'settings.updates.title', defaultMessage: 'Updates' },
  updatesDesc: {
    id: 'settings.updates.description',
    defaultMessage: 'Check for and install updates to keep goose running at its best',
  },
  notificationsModalTitle: {
    id: 'settings.notifications.modal.title',
    defaultMessage: 'How to Enable Notifications',
  },
  notificationsMacInstructions: {
    id: 'settings.notifications.modal.macInstructions',
    defaultMessage: 'To enable notifications on macOS:',
  },
  notificationsMacStep1: {
    id: 'settings.notifications.modal.macStep1',
    defaultMessage: 'Open System Preferences',
  },
  notificationsMacStep2: {
    id: 'settings.notifications.modal.macStep2',
    defaultMessage: 'Click on Notifications',
  },
  notificationsMacStep3: {
    id: 'settings.notifications.modal.macStep3',
    defaultMessage: 'Find and select goose in the application list',
  },
  notificationsMacStep4: {
    id: 'settings.notifications.modal.macStep4',
    defaultMessage: 'Enable notifications and adjust settings as desired',
  },
  notificationsWinInstructions: {
    id: 'settings.notifications.modal.winInstructions',
    defaultMessage: 'To enable notifications on Windows:',
  },
  notificationsWinStep1: {
    id: 'settings.notifications.modal.winStep1',
    defaultMessage: 'Open Settings',
  },
  notificationsWinStep2: {
    id: 'settings.notifications.modal.winStep2',
    defaultMessage: 'Go to System > Notifications',
  },
  notificationsWinStep3: {
    id: 'settings.notifications.modal.winStep3',
    defaultMessage: 'Find and select goose in the application list',
  },
  notificationsWinStep4: {
    id: 'settings.notifications.modal.winStep4',
    defaultMessage: 'Toggle notifications on and adjust settings as desired',
  },
  close: { id: 'settings.close', defaultMessage: 'Close' },
});

const LANGUAGE_OPTIONS = [
  { value: 'system', message: 'languageSystem' },
  { value: 'en', message: 'languageEnglish' },
  { value: 'es', message: 'languageSpanish' },
  { value: 'fr', message: 'languageFrench' },
  { value: 'de', message: 'languageGerman' },
  { value: 'it', message: 'languageItalian' },
  { value: 'pt', message: 'languagePortuguese' },
  { value: 'id', message: 'languageIndonesian' },
  { value: 'ms', message: 'languageMalay' },
  { value: 'vi', message: 'languageVietnamese' },
  { value: 'hi', message: 'languageHindi' },
  { value: 'ja', message: 'languageJapanese' },
  { value: 'ko', message: 'languageKorean' },
  { value: 'ru', message: 'languageRussian' },
  { value: 'tr', message: 'languageTurkish' },
  { value: 'zh-CN', message: 'languageChineseSimplified' },
  { value: 'zh-TW', message: 'languageChineseTraditional' },
] as const satisfies ReadonlyArray<{
  value: LanguageSetting;
  message: keyof typeof i18n;
}>;

type LanguageOption = (typeof LANGUAGE_OPTIONS)[number];

export default function AppSettingsSection({
  scrollToSection,
}: AppSettingsSectionProps): JSX.Element {
  const [menuBarIconEnabled, setMenuBarIconEnabled] = useState<boolean>(true);
  const [dockIconEnabled, setDockIconEnabled] = useState<boolean>(true);
  const [wakelockEnabled, setWakelockEnabled] = useState<boolean>(true);
  const [notificationsEnabled, setNotificationsEnabled] = useState<boolean>(true);
  const [isMacOS] = useState<boolean>(window.electron.platform === 'darwin');
  const [isDockSwitchDisabled, setIsDockSwitchDisabled] = useState<boolean>(false);
  const [showNotificationModal, setShowNotificationModal] = useState<boolean>(false);
  const [showPricing, setShowPricing] = useState<boolean>(true);
  const [language, setLanguage] = useState<LanguageSetting>('system');
  const [isDarkMode, setIsDarkMode] = useState<boolean>(false);
  const updateSectionRef = useRef<HTMLDivElement | null>(null);
  const gooseVersion: unknown = window.appConfig.get('BCAIP_VERSION');
  const shouldShowUpdates: boolean = !gooseVersion;
  const displayedVersion: string = typeof gooseVersion === 'string' ? gooseVersion : 'Development';

  useEffect(() => {
    const updateTheme = (): void => {
      setIsDarkMode(document.documentElement.classList.contains('dark'));
    };

    updateTheme();

    const observer: MutationObserver = new MutationObserver(updateTheme);
    observer.observe(document.documentElement, {
      attributes: true,
      attributeFilter: ['class'],
    });

    return (): void => observer.disconnect();
  }, []);

  useEffect(() => {
    void window.electron.getSetting('showPricing').then((value): void => {
      setShowPricing(value);
    });
    void window.electron.getSetting('language').then((value): void => {
      setLanguage(isLanguageSetting(value) ? value : 'system');
    });
  }, []);

  useEffect(() => {
    if (scrollToSection === 'update' && updateSectionRef.current) {
      setTimeout((): void => {
        updateSectionRef.current?.scrollIntoView({ behavior: 'smooth', block: 'center' });
      }, 100);
    }
  }, [scrollToSection]);

  useEffect(() => {
    void window.electron.getMenuBarIconState().then((enabled): void => {
      setMenuBarIconEnabled(enabled);
    });

    void window.electron.getWakelockState().then((enabled): void => {
      setWakelockEnabled(enabled);
    });

    void window.electron.getSetting('enableNotifications').then((enabled): void => {
      setNotificationsEnabled(enabled ?? true);
    });

    if (isMacOS) {
      void window.electron.getDockIconState().then((enabled): void => {
        setDockIconEnabled(enabled);
      });
    }
  }, [isMacOS]);

  const handleMenuBarIconToggle = async (): Promise<void> => {
    const newState: boolean = !menuBarIconEnabled;
    // If we're turning off the menu bar icon and the dock icon is hidden,
    // we need to show the dock icon to maintain accessibility
    if (!newState && !dockIconEnabled && isMacOS) {
      const success: boolean = await window.electron.setDockIcon(true);
      if (success) {
        setDockIconEnabled(true);
      }
    }
    const success: boolean = await window.electron.setMenuBarIcon(newState);
    if (success) {
      setMenuBarIconEnabled(newState);
      trackSettingToggled('menu_bar_icon', newState);
    }
  };

  const handleDockIconToggle = async (): Promise<void> => {
    const newState: boolean = !dockIconEnabled;
    // If we're turning off the dock icon and the menu bar icon is hidden,
    // we need to show the menu bar icon to maintain accessibility
    if (!newState && !menuBarIconEnabled) {
      const success: boolean = await window.electron.setMenuBarIcon(true);
      if (success) {
        setMenuBarIconEnabled(true);
      }
    }

    // Disable the switch to prevent rapid toggling
    setIsDockSwitchDisabled(true);
    setTimeout((): void => {
      setIsDockSwitchDisabled(false);
    }, 1000);

    // Set the dock icon state
    const success: boolean = await window.electron.setDockIcon(newState);
    if (success) {
      setDockIconEnabled(newState);
      trackSettingToggled('dock_icon', newState);
    }
  };

  const handleWakelockToggle = async (): Promise<void> => {
    const newState: boolean = !wakelockEnabled;
    const success: boolean = await window.electron.setWakelock(newState);
    if (success) {
      setWakelockEnabled(newState);
      trackSettingToggled('prevent_sleep', newState);
    }
  };

  const handleNotificationsToggle = async (checked: boolean): Promise<void> => {
    setNotificationsEnabled(checked);
    await window.electron.setSetting('enableNotifications', checked);
    trackSettingToggled('task_notifications', checked);
  };

  const handleShowPricingToggle = async (checked: boolean): Promise<void> => {
    setShowPricing(checked);
    await window.electron.setSetting('showPricing', checked);
    trackSettingToggled('cost_tracking', checked);
    // Trigger event for other components
    window.dispatchEvent(new CustomEvent('showPricingChanged'));
  };

  const handleLanguageChange = async (value: string): Promise<void> => {
    const nextLanguage: LanguageSetting | undefined = LANGUAGE_OPTIONS.find(
      (option: LanguageOption): boolean => option.value === value
    )?.value;
    if (!nextLanguage || nextLanguage === language) {
      return;
    }

    setLanguage(nextLanguage);
    try {
      await window.electron.setSetting('language', nextLanguage);
      window.electron.reloadApp();
    } catch (error) {
      console.error('Failed to update language setting:', error);
      setLanguage(language);
    }
  };

  const intl: ReturnType<typeof useIntl> = useIntl();
  const selectedLanguage: LanguageOption =
    LANGUAGE_OPTIONS.find((option: LanguageOption): boolean => option.value === language) ??
    LANGUAGE_OPTIONS[0];

  return (
    <div className="space-y-4 pr-4 pb-8 mt-1">
      <Card className="rounded-lg">
        <CardHeader className="pb-0">
          <CardTitle className="">{intl.formatMessage(i18n.appearanceTitle)}</CardTitle>
          <CardDescription>{intl.formatMessage(i18n.appearanceDesc)}</CardDescription>
        </CardHeader>
        <CardContent className="pt-4 space-y-4 px-4">
          <div className="flex items-center justify-between">
            <div>
              <h3 className="text-text-primary text-xs">
                {intl.formatMessage(i18n.notifications)}
              </h3>
              <p className="text-xs text-text-secondary max-w-md mt-[2px]">
                {intl.formatMessage(i18n.notificationsDesc, {
                  link: (
                    <span
                      className="underline hover:cursor-pointer"
                      onClick={() => setShowNotificationModal(true)}
                    >
                      {intl.formatMessage(i18n.configGuide)}
                    </span>
                  ),
                })}
              </p>
            </div>
            <div className="flex items-center">
              <Button
                className="flex items-center gap-2 justify-center"
                variant="secondary"
                size="sm"
                onClick={(): void => {
                  void (async (): Promise<void> => {
                    try {
                      await window.electron.openNotificationsSettings();
                    } catch (error) {
                      console.error('Failed to open notification settings:', error);
                    }
                  })();
                }}
              >
                <Settings />
                {intl.formatMessage(i18n.openSettings)}
              </Button>
            </div>
          </div>

          <div className="flex items-center justify-between">
            <div>
              <h3 className="text-text-primary text-xs">
                {intl.formatMessage(i18n.taskNotifications)}
              </h3>
              <p className="text-xs text-text-secondary max-w-md mt-[2px]">
                {intl.formatMessage(i18n.taskNotificationsDesc)}
              </p>
            </div>
            <div className="flex items-center">
              <Switch
                checked={notificationsEnabled}
                onCheckedChange={(checked: boolean): void => {
                  void handleNotificationsToggle(checked);
                }}
                variant="mono"
              />
            </div>
          </div>

          <div className="flex items-center justify-between">
            <div>
              <h3 className="text-text-primary text-xs">{intl.formatMessage(i18n.menuBarIcon)}</h3>
              <p className="text-xs text-text-secondary max-w-md mt-[2px]">
                {intl.formatMessage(i18n.menuBarIconDesc)}
              </p>
            </div>
            <div className="flex items-center">
              <Switch
                checked={menuBarIconEnabled}
                onCheckedChange={(): void => {
                  void handleMenuBarIconToggle();
                }}
                variant="mono"
              />
            </div>
          </div>

          {isMacOS && (
            <div className="flex items-center justify-between">
              <div>
                <h3 className="text-text-primary text-xs">{intl.formatMessage(i18n.dockIcon)}</h3>
                <p className="text-xs text-text-secondary max-w-md mt-[2px]">
                  {intl.formatMessage(i18n.dockIconDesc)}
                </p>
              </div>
              <div className="flex items-center">
                <Switch
                  disabled={isDockSwitchDisabled}
                  checked={dockIconEnabled}
                  onCheckedChange={(): void => {
                    void handleDockIconToggle();
                  }}
                  variant="mono"
                />
              </div>
            </div>
          )}

          {/* Prevent Sleep */}
          <div className="flex items-center justify-between">
            <div>
              <h3 className="text-text-primary text-xs">{intl.formatMessage(i18n.preventSleep)}</h3>
              <p className="text-xs text-text-secondary max-w-md mt-[2px]">
                {intl.formatMessage(i18n.preventSleepDesc)}
              </p>
            </div>
            <div className="flex items-center">
              <Switch
                checked={wakelockEnabled}
                onCheckedChange={(): void => {
                  void handleWakelockToggle();
                }}
                variant="mono"
              />
            </div>
          </div>

          {/* Cost Tracking */}
          {COST_TRACKING_ENABLED && (
            <div className="flex items-center justify-between mb-4">
              <div>
                <h3 className="text-text-primary">{intl.formatMessage(i18n.costTracking)}</h3>
                <p className="text-xs text-text-secondary max-w-md mt-[2px]">
                  {intl.formatMessage(i18n.costTrackingDesc)}
                </p>
              </div>
              <div className="flex items-center">
                <Switch
                  checked={showPricing}
                  onCheckedChange={(checked: boolean): void => {
                    void handleShowPricingToggle(checked);
                  }}
                  variant="mono"
                />
              </div>
            </div>
          )}
        </CardContent>
      </Card>

      <Card className="rounded-lg">
        <CardHeader className="pb-0">
          <CardTitle className="mb-1">{intl.formatMessage(i18n.themeTitle)}</CardTitle>
          <CardDescription>{intl.formatMessage(i18n.themeDesc)}</CardDescription>
        </CardHeader>
        <CardContent className="pt-4 px-4">
          <ThemeSelector className="w-auto" hideTitle horizontal />
        </CardContent>
      </Card>

      <Card className="rounded-lg">
        <CardHeader className="pb-0">
          <CardTitle className="mb-1">{intl.formatMessage(i18n.languageTitle)}</CardTitle>
          <CardDescription>{intl.formatMessage(i18n.languageDesc)}</CardDescription>
        </CardHeader>
        <CardContent className="pt-4 px-4">
          <DropdownMenu>
            <DropdownMenuTrigger className="flex w-full max-w-[260px] items-center justify-between gap-2 rounded-md border border-border-primary bg-background-primary px-3 py-2 text-sm text-text-primary transition-colors hover:border-border-primary">
              <span className="truncate">{intl.formatMessage(i18n[selectedLanguage.message])}</span>
              <ChevronDown className="h-4 w-4 shrink-0" />
            </DropdownMenuTrigger>
            <DropdownMenuContent align="start" className="w-[260px]">
              <DropdownMenuRadioGroup value={language} onValueChange={handleLanguageChange}>
                {LANGUAGE_OPTIONS.map((option) => (
                  <DropdownMenuRadioItem key={option.value} value={option.value}>
                    {intl.formatMessage(i18n[option.message])}
                  </DropdownMenuRadioItem>
                ))}
              </DropdownMenuRadioGroup>
            </DropdownMenuContent>
          </DropdownMenu>
        </CardContent>
      </Card>
      <TelemetrySettings />

      <Card className="rounded-lg">
        <CardHeader className="pb-0">
          <CardTitle className="mb-1">{intl.formatMessage(i18n.helpTitle)}</CardTitle>
          <CardDescription>{intl.formatMessage(i18n.helpDesc)}</CardDescription>
        </CardHeader>
        <CardContent className="pt-4 px-4">
          <div className="flex space-x-4">
            <Button
              onClick={() => {
                window.open(
                  'https://github.com/BezotCorp/ai-platform/issues/new?template=bug_report.md',
                  '_blank'
                );
              }}
              variant="secondary"
              size="sm"
            >
              {intl.formatMessage(i18n.reportBug)}
            </Button>
            <Button
              onClick={() => {
                window.open(
                  'https://github.com/BezotCorp/ai-platform/issues/new?template=feature_request.md',
                  '_blank'
                );
              }}
              variant="secondary"
              size="sm"
            >
              {intl.formatMessage(i18n.requestFeature)}
            </Button>
          </div>
        </CardContent>
      </Card>

      {/* Version Section - only show if BCAIP_VERSION is set */}
      {!shouldShowUpdates && (
        <Card className="rounded-lg">
          <CardHeader className="pb-0">
            <CardTitle className="mb-1">{intl.formatMessage(i18n.versionTitle)}</CardTitle>
          </CardHeader>
          <CardContent className="pt-4 px-4">
            <div className="flex items-center gap-3">
              <img
                src={isDarkMode ? BlockLogoWhite : BlockLogoBlack}
                alt="Block Logo" // TODO: replace with AAIF logo asset
                className="h-8 w-auto"
              />
              <span className="text-2xl font-mono text-black dark:text-white">
                {displayedVersion}
              </span>
            </div>
          </CardContent>
        </Card>
      )}

      {/* Update Section - only show if BCAIP_VERSION is NOT set */}
      {UPDATES_ENABLED && shouldShowUpdates && (
        <div ref={updateSectionRef}>
          <Card className="rounded-lg">
            <CardHeader className="pb-0">
              <CardTitle className="mb-1">{intl.formatMessage(i18n.updatesTitle)}</CardTitle>
              <CardDescription>{intl.formatMessage(i18n.updatesDesc)}</CardDescription>
            </CardHeader>
            <CardContent className="px-4">
              <UpdateSection />
            </CardContent>
          </Card>
        </div>
      )}

      {/* Notification Instructions Modal */}
      <Dialog
        open={showNotificationModal}
        onOpenChange={(open: boolean): void => {
          if (!open) {
            setShowNotificationModal(false);
          }
        }}
      >
        <DialogContent className="sm:max-w-[500px]">
          <DialogHeader>
            <DialogTitle className="flex items-center gap-2">
              <Settings className="text-iconStandard" size={24} />
              {intl.formatMessage(i18n.notificationsModalTitle)}
            </DialogTitle>
          </DialogHeader>

          <div className="py-4">
            {/* OS-specific instructions */}
            {isMacOS ? (
              <div className="space-y-4">
                <p>{intl.formatMessage(i18n.notificationsMacInstructions)}</p>
                <ol className="list-decimal pl-5 space-y-2">
                  <li>{intl.formatMessage(i18n.notificationsMacStep1)}</li>
                  <li>{intl.formatMessage(i18n.notificationsMacStep2)}</li>
                  <li>{intl.formatMessage(i18n.notificationsMacStep3)}</li>
                  <li>{intl.formatMessage(i18n.notificationsMacStep4)}</li>
                </ol>
              </div>
            ) : (
              <div className="space-y-4">
                <p>{intl.formatMessage(i18n.notificationsWinInstructions)}</p>
                <ol className="list-decimal pl-5 space-y-2">
                  <li>{intl.formatMessage(i18n.notificationsWinStep1)}</li>
                  <li>{intl.formatMessage(i18n.notificationsWinStep2)}</li>
                  <li>{intl.formatMessage(i18n.notificationsWinStep3)}</li>
                  <li>{intl.formatMessage(i18n.notificationsWinStep4)}</li>
                </ol>
              </div>
            )}
          </div>

          <DialogFooter>
            <Button variant="outline" onClick={() => setShowNotificationModal(false)}>
              {intl.formatMessage(i18n.close)}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}

function isLanguageSetting(value: unknown): value is LanguageSetting {
  return LANGUAGE_OPTIONS.some((option: LanguageOption): boolean => option.value === value);
}
