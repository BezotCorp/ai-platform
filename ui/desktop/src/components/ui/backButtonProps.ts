import type { VariantProps } from 'class-variance-authority';
import { buttonVariants } from './button';

export interface BackButtonProps extends VariantProps<typeof buttonVariants> {
  onClick?: () => void;
  className?: string;
  showText?: boolean;
  shape?: 'pill' | 'round';
}
