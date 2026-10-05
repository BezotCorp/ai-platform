export interface RecorderGeneration {
  audioContext: AudioContext | null;
  cancelled: boolean;
  completesAfterTranscription: boolean;
  pendingTranscriptions: number;
  stream: MediaStream | null;
  worklet: AudioWorkletNode | null;
}
