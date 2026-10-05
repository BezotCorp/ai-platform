import type { SearchHighlighter } from '../../../utils/searchHighlighter';

export interface SearchContainerElement extends HTMLDivElement {
  _searchHighlighter: SearchHighlighter | null;
}
