/**
 * Markdown — render GitHub-flavored markdown safely.
 *
 * Wraps `react-markdown` + `remark-gfm`. `react-markdown` does not
 * use `dangerouslySetInnerHTML`; it parses to a vdom tree and
 * escapes raw HTML by default, so the issue body (sourced from
 * GitHub) cannot inject script tags.
 *
 * Styling is intentionally minimal and inline (Tailwind utility
 * classes via component overrides) so we don't need the
 * `@tailwindcss/typography` plugin.
 */

import ReactMarkdown, { type Components } from "react-markdown";
import remarkGfm from "remark-gfm";

import { cn } from "@/lib/utils";

const components: Components = {
  h1: ({ className, ...props }) => (
    <h1 className={cn("mt-4 mb-2 text-lg font-semibold first:mt-0", className)} {...props} />
  ),
  h2: ({ className, ...props }) => (
    <h2 className={cn("mt-4 mb-2 text-base font-semibold first:mt-0", className)} {...props} />
  ),
  h3: ({ className, ...props }) => (
    <h3 className={cn("mt-3 mb-1.5 text-sm font-semibold first:mt-0", className)} {...props} />
  ),
  h4: ({ className, ...props }) => (
    <h4 className={cn("mt-3 mb-1 text-sm font-semibold first:mt-0", className)} {...props} />
  ),
  p: ({ className, ...props }) => (
    <p className={cn("my-2 text-sm leading-relaxed first:mt-0 last:mb-0", className)} {...props} />
  ),
  ul: ({ className, ...props }) => (
    <ul className={cn("my-2 ml-5 list-disc space-y-1 text-sm", className)} {...props} />
  ),
  ol: ({ className, ...props }) => (
    <ol className={cn("my-2 ml-5 list-decimal space-y-1 text-sm", className)} {...props} />
  ),
  li: ({ className, ...props }) => (
    <li className={cn("leading-relaxed", className)} {...props} />
  ),
  a: ({ className, ...props }) => (
    <a
      className={cn("text-primary underline underline-offset-2 hover:opacity-80", className)}
      target="_blank"
      rel="noreferrer"
      {...props}
    />
  ),
  code: ({ className, ...props }) => (
    <code
      className={cn(
        "rounded bg-muted px-1 py-0.5 font-mono text-[0.85em]",
        className,
      )}
      {...props}
    />
  ),
  pre: ({ className, ...props }) => (
    <pre
      className={cn(
        "my-2 overflow-x-auto rounded-md bg-muted p-3 font-mono text-xs",
        className,
      )}
      {...props}
    />
  ),
  blockquote: ({ className, ...props }) => (
    <blockquote
      className={cn("my-2 border-l-2 border-border pl-3 text-muted-foreground", className)}
      {...props}
    />
  ),
  hr: ({ className, ...props }) => (
    <hr className={cn("my-3 border-border", className)} {...props} />
  ),
  table: ({ className, ...props }) => (
    <div className="my-2 overflow-x-auto">
      <table className={cn("w-full border-collapse text-sm", className)} {...props} />
    </div>
  ),
  th: ({ className, ...props }) => (
    <th
      className={cn("border border-border bg-muted px-2 py-1 text-left font-medium", className)}
      {...props}
    />
  ),
  td: ({ className, ...props }) => (
    <td className={cn("border border-border px-2 py-1 align-top", className)} {...props} />
  ),
  input: ({ className, type, ...props }) => {
    // GFM task list items render as `<input type="checkbox" disabled />`.
    if (type === "checkbox") {
      return (
        <input
          type="checkbox"
          className={cn("mr-1 align-middle", className)}
          {...props}
          disabled
        />
      );
    }
    return <input type={type} className={className} {...props} />;
  },
};

export function Markdown({
  children,
  className,
}: {
  children: string;
  className?: string;
}): JSX.Element {
  return (
    <div className={cn("text-foreground", className)} data-testid="markdown">
      <ReactMarkdown remarkPlugins={[remarkGfm]} components={components}>
        {children}
      </ReactMarkdown>
    </div>
  );
}
