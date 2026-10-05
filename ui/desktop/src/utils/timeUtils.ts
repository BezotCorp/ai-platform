import type { ClockDisplay } from './clockDisplay';
export type { ClockDisplay } from './clockDisplay';

import { currentLocale } from '../i18n';
import { AppDate } from './appDate';

export function formatMessageTimestamp(timestamp?: number): string {
  const date: AppDate = timestamp
    ? AppDate.fromTimestampSeconds(timestamp)
    : AppDate.now();
  const now: AppDate = AppDate.now();
  const timeStr: string = date.toLocaleTimeString(currentLocale, {
    hour: 'numeric',
    minute: '2-digit',
  });
  if (date.isSameDay(now)) {
    return timeStr;
  }
  const dateStr: string = date.toLocaleDateString(currentLocale, {
    month: '2-digit',
    day: '2-digit',
    year: 'numeric',
  });
  return `${dateStr} ${timeStr}`;
}


export function formatClockDisplay(
  date: AppDate = AppDate.now(),
  locale: string = currentLocale
): ClockDisplay {
  const hour: number = date.getHours();
  try {
    const parts: Intl.DateTimeFormatPart[] = date.formatToParts(locale, {
      hour: 'numeric',
      minute: '2-digit',
    });
    const dayPeriodPart: Intl.DateTimeFormatPart | undefined = parts.find(
      (part: Intl.DateTimeFormatPart): boolean => part.type === 'dayPeriod'
    );
    const meridiem: string = dayPeriodPart?.value ?? '';
    const time: string = parts
      .filter((part: Intl.DateTimeFormatPart): boolean => part.type !== 'dayPeriod')
      .map((part: Intl.DateTimeFormatPart): string => part.value)
      .join('')
      .trim();
    return { time, meridiem, hour };
  } catch {
    const minutes: number = date.getMinutes();
    const meridiem: string = hour >= 12 ? 'PM' : 'AM';
    const displayHour: number = ((hour + 11) % 12) + 1;
    const time: string = `${displayHour}:${String(minutes).padStart(2, '0')}`;
    return { time, meridiem, hour };
  }
}
