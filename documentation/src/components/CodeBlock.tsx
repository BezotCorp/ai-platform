import type { PropsWithChildren } from "react";

type CodeBlockProps = PropsWithChildren<{
  language?: string;
  title?: string;
}>;

export default function CodeBlock({
  language,
  title,
  children,
}: CodeBlockProps) {
  return (
    <div className="documentation-code-block">
      {title ? (
        <div className="documentation-code-block__title">
          {title}
        </div>
      ) : null}

      <pre>
        <code
          className={
            language
              ? `language-${language}`
              : undefined
          }
        >
          {children}
        </code>
      </pre>
    </div>
  );
}
