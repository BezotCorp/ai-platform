import React from 'react';

export interface SearchBarProps {
  /** Callback fired when search term or case sensitivity changes */
  onSearch: (term: string, caseSensitive: boolean) => void;
  /** Callback fired when the search bar is closed */
  onClose: () => void;
  /** Optional callback for navigating between search results */
  onNavigate?: (direction: 'next' | 'prev') => void;
  /** Current search results state */
  searchResults?: {
    count: number;
    currentIndex: number;
  };
  /** Optional ref for the search input element */
  inputRef?: React.RefObject<HTMLInputElement>;
  /** Initial search term */
  initialSearchTerm?: string;
  /** Placeholder text for the search input */
  placeholder?: string;
  /** Show the case-sensitivity toggle (default: true). */
  showCaseSensitive?: boolean;
  /** Show the previous/next match navigation arrows (default: true). When hidden, the
   * result counter shows the total instead of "current/total". */
  showNavigation?: boolean;
}
