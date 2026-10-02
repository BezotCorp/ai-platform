import type { SessionListItem } from '../acp/sessionListItem';
import { AppDate } from './appDate';

export interface DateGroup {
  label: string;
  sessions: SessionListItem[];
  date: AppDate;
}

export function groupSessionsByDate(sessions: SessionListItem[]): DateGroup[] {
  const today: AppDate = AppDate.now().startOfDay();
  const yesterday: AppDate = today.addDays(-1);

  const groups: Record<string, DateGroup> = {};

  sessions.forEach((session: SessionListItem): void => {
    const sessionDateStart: AppDate = session.activityAt.startOfDay();

    let label: string;
    let groupKey: string;

    if (sessionDateStart.isSameDay(today)) {
      label = 'Today';
      groupKey = 'today';
    } else if (sessionDateStart.isSameDay(yesterday)) {
      label = 'Yesterday';
      groupKey = 'yesterday';
    } else {
      if (sessionDateStart.getYear() === today.getYear()) {
        label = sessionDateStart.toLocaleDateString('en-US', {
          weekday: 'long',
          month: 'long',
          day: 'numeric',
        });
      } else {
        label = sessionDateStart.toLocaleDateString('en-US', {
          month: 'long',
          day: 'numeric',
          year: 'numeric',
        });
      }

      groupKey = sessionDateStart.toLocalDateKey();
    }

    if (!groups[groupKey]) {
      groups[groupKey] = {
        label,
        sessions: [],
        date: sessionDateStart,
      };
    }

    groups[groupKey].sessions.push(session);
  });

  return Object.values(groups).sort(
    (a: DateGroup, b: DateGroup): number => b.date.getTime() - a.date.getTime()
  );
}
