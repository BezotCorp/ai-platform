import type { InterruptionKeyword } from './interruptionDetector';
import { INTERRUPTION_KEYWORDS } from './interruptionDetector';

export interface InterruptionMatch {
  keyword: InterruptionKeyword;
  matchedText: string;
  confidence: number;
  shouldInterrupt: boolean;
}

/**
 * Analyzes input text for interruption keywords
 */
export function detectInterruption(input: string): InterruptionMatch | null {
  if (!input || input.trim().length === 0) {
    return null;
  }
  const normalizedInput = input.toLowerCase().trim();
  // Check for exact matches first (highest confidence)
  for (const keyword of INTERRUPTION_KEYWORDS) {
    for (const variation of keyword.variations) {
      if (normalizedInput === variation) {
        return {
          keyword,
          matchedText: variation,
          confidence: 1.0,
          shouldInterrupt: true,
        };
      }
    }
  }
  // Check for matches at the beginning of input (high confidence)
  for (const keyword of INTERRUPTION_KEYWORDS) {
    for (const variation of keyword.variations) {
      if (
        normalizedInput.startsWith(variation + ' ') ||
        normalizedInput.startsWith(variation + ',')
      ) {
        return {
          keyword,
          matchedText: variation,
          confidence: 0.9,
          shouldInterrupt: true,
        };
      }
    }
  }
  // Check for matches anywhere in short inputs (medium confidence)
  if (normalizedInput.length <= 20) {
    for (const keyword of INTERRUPTION_KEYWORDS) {
      for (const variation of keyword.variations) {
        if (normalizedInput.includes(variation)) {
          return {
            keyword,
            matchedText: variation,
            confidence: 0.7,
            shouldInterrupt: keyword.priority === 'high',
          };
        }
      }
    }
  }
  return null;
}

/**
 * Checks if input is likely an interruption command
 */
export function isInterruptionCommand(input: string): boolean {
  const match = detectInterruption(input);
  return match?.shouldInterrupt ?? false;
}

/**
 * Gets a user-friendly message for the interruption action
 */
export function getInterruptionMessage(match: InterruptionMatch): string {
  switch (match.keyword.action) {
    case 'stop':
      return `Stopped processing. You said "${match.matchedText}".`;
    case 'pause':
      return `Paused processing. You said "${match.matchedText}".`;
    case 'redirect':
      return `Stopping to redirect. You said "${match.matchedText}".`;
    default:
      return `Interrupted processing. You said "${match.matchedText}".`;
  }
}
