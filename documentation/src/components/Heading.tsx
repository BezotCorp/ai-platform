import type {
  HTMLAttributes,
  PropsWithChildren,
} from "react";

type HeadingLevel =
  | "h1"
  | "h2"
  | "h3"
  | "h4"
  | "h5"
  | "h6";

type HeadingProps = PropsWithChildren<
  HTMLAttributes<HTMLHeadingElement> & {
    as?: HeadingLevel;
  }
>;

export default function Heading({
  as: Element = "h2",
  children,
  ...props
}: HeadingProps) {
  return <Element {...props}>{children}</Element>;
}
