export interface InterruptionKeyword {
  keyword: string;
  variations: string[];
  priority: 'high' | 'medium' | 'low';
  action: 'stop' | 'pause' | 'redirect';
}
