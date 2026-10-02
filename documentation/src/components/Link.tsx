import type {
  AnchorHTMLAttributes,
  PropsWithChildren,
} from "react";

type LinkProps = PropsWithChildren<
  AnchorHTMLAttributes<HTMLAnchorElement> & {
    to?: string;
  }
>;

export default function Link({
  to,
  href,
  children,
  ...props
}: LinkProps) {
  return (
    <a href={href ?? to} {...props}>
      {children}
    </a>
  );
}
