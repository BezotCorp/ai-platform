/**
 * Utility for detecting interruption keywords in user input
 */
export interface InterruptionKeyword {
  keyword: string;
  variations: string[];
  priority: 'high' | 'medium' | 'low';
  action: 'stop' | 'pause' | 'redirect';
}

// Define interruption keywords and their variations
export const INTERRUPTION_KEYWORDS: InterruptionKeyword[] = [
  {
    keyword: 'stop',
    variations: ['stop', 'halt', 'cease', 'quit', 'end', 'abort', 'cancel'],
    priority: 'high',
    action: 'stop',
  },
  {
    keyword: 'wait',
    variations: ['wait', 'hold', 'pause', 'hold on', 'wait up', 'hold up'],
    priority: 'high',
    action: 'pause',
  },
  {
    keyword: 'no',
    variations: ['no', 'nope', 'nah', 'wrong', 'incorrect', 'not right'],
    priority: 'medium',
    action: 'stop',
  },
  {
    keyword: 'actually',
    variations: ['actually', 'instead', 'rather', 'better idea', 'change of plans'],
    priority: 'medium',
    action: 'redirect',
  },
  {
    keyword: 'nevermind',
    variations: ['nevermind', 'never mind', 'forget it', 'ignore that', 'disregard'],
    priority: 'medium',
    action: 'stop',
  },
];
