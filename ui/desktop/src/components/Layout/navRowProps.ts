import type { NavItem } from '../../hooks/useNavigationItems';

export interface NavRowProps {
  item: NavItem;
  active: boolean;
  onClick: () => void;
}
