import { AppDate } from './appDate';

export const formatToLocalDateWithTimezone = (dateString?: string | null): string => {
  if (!dateString) {
    return 'N/A';
  }
  try {
    return AppDate.fromString(dateString).toLocaleString(undefined, {
      year: 'numeric',
      month: 'short',
      day: 'numeric',
      hour: 'numeric',
      minute: '2-digit',
      second: '2-digit',
      timeZoneName: 'short',
    });
  } catch (error) {
    console.error('Error formatting date with timezone:', error);
    return 'Invalid Date';
  }
};
