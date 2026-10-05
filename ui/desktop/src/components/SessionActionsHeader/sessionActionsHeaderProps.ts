import type { Session } from '../../types/session';

export interface SessionActionsHeaderProps {
  session?: Session;
  onSessionChange: (updater: (session: Session) => Session) => void;
  className?: string;
}
