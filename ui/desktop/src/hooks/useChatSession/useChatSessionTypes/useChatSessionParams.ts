export interface UseChatSessionParams {
  sessionId: string;
  onStreamFinish: () => void;
  onSessionLoaded?: () => void;
}
