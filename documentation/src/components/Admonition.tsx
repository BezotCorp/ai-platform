import type { PropsWithChildren } from "react";

type AdmonitionProps = PropsWithChildren<{
  type?: "note" | "tip" | "info" | "warning" | "danger";
  title?: string;
}>;

export default function Admonition({
  type = "note",
  title,
  children,
}: AdmonitionProps) {
  return (
    <aside
      className={`documentation-admonition documentation-admonition--${type}`}
    >
      {title ? <strong>{title}</strong> : null}
      {children}
    </aside>
  );
}
