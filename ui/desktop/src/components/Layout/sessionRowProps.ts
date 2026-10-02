import { SessionListItem } from '../../acp/sessionListItem';
import { SessionStatus } from './sessionStatus';

export interface SessionRowProps {
  session: SessionListItem;
  active: boolean;
  isLiveVoiceActive: boolean;
  status: SessionStatus | undefined;
  onClick: () => void;
  onRenamed: () => void;
}
