export interface SearchViewProps {
  /** Optional CSS class name */
  className?: string;
  /** Optional callback for search term changes */
  onSearch?: (term: string, caseSensitive: boolean) => void;
  /** Optional callback for navigating between search results */
  onNavigate?: (direction: 'next' | 'prev') => void;
  /** Current search results state */
  searchResults?: {
    count: number;
    currentIndex: number;
  } | null;
  /** Placeholder text for the search input */
  placeholder?: string;
  /** Show the case-sensitivity toggle (default: true). */
  showCaseSensitive?: boolean;
  /** Show the previous/next match navigation arrows (default: true). */
  showNavigation?: boolean;
  /** Highlight matches in the content via the find-in-page highlighter (default: true).
   * Set false when the search acts as a list filter, so the term only drives onSearch. */
  highlightMatches?: boolean;
}
