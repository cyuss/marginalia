/** Shared page heading. Keeps typography and spacing consistent per screen. */
import type { ReactNode } from "react";

export function ScreenHeader({
  title,
  description,
}: {
  title: string;
  description?: string;
}) {
  return (
    <header className="border-b border-[var(--rule)] pb-5">
      <h1 className="text-xl tracking-tight text-[var(--ink)]">{title}</h1>
      {description ? (
        <p className="ui prose-measure mt-1.5 text-sm text-[var(--ink-muted)]">
          {description}
        </p>
      ) : null}
    </header>
  );
}

export function Screen({ children }: { children: ReactNode }) {
  return <div className="space-y-2">{children}</div>;
}
