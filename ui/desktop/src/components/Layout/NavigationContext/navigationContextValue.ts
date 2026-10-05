export interface NavigationContextValue {
  isNavExpanded: boolean;
  setIsNavExpanded: (expanded: boolean) => void;
  navWidth: number;
  setNavWidth: (width: number) => void;
}
