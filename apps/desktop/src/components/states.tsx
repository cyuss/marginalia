/**
 * Loading, empty and error states.
 *
 * WHY these are shared components rather than ad-hoc JSX: the definition of
 * done requires all three for every screen, and a shared component is the only
 * way that survives contact with a deadline.
 *
 * The error component takes the four questions from the error model as
 * *required* props — you cannot render an error in Marginalia without saying
 * what happened, what was affected, and whether the user's data is safe.
 */

import type { ReactNode } from "react";

export function LoadingState({ label }: { label: string }) {
  return (
    <div className="ui flex items-center gap-3 py-16 text-[var(--ink-faint)]">
      <span
        aria-hidden
        className="h-1.5 w-1.5 animate-pulse rounded-full bg-[var(--ink-faint)]"
      />
      <span className="text-sm">{label}</span>
    </div>
  );
}

export function EmptyState({
  title,
  description,
  action,
}: {
  title: string;
  description: string;
  action?: ReactNode;
}) {
  return (
    <div className="prose-measure py-16">
      <h2 className="text-lg text-[var(--ink)]">{title}</h2>
      <p className="ui mt-2 text-sm leading-relaxed text-[var(--ink-muted)]">
        {description}
      </p>
      {action ? <div className="mt-6">{action}</div> : null}
    </div>
  );
}

export interface ErrorStateProps {
  /** What happened, in one sentence, in plain language. */
  whatHappened: string;
  /** What was touched. */
  whatWasAffected: string;
  /** Whether the user's data is intact. Almost always true — say so. */
  dataIsSafe: boolean;
  /** The concrete next step, if there is one. */
  remediation?: string;
  onRetry?: () => void;
  onDetails?: () => void;
}

export function ErrorState({
  whatHappened,
  whatWasAffected,
  dataIsSafe,
  remediation,
  onRetry,
  onDetails,
}: ErrorStateProps) {
  return (
    <div
      role="alert"
      className="prose-measure rounded-[var(--radius)] border border-[var(--rule-strong)] bg-[var(--paper-raised)] p-6"
    >
      <h2 className="text-base text-[var(--ink)]">{whatHappened}</h2>

      <p className="ui mt-3 text-sm leading-relaxed text-[var(--ink-muted)]">
        {whatWasAffected}
      </p>

      {dataIsSafe ? (
        <p className="ui mt-2 text-sm leading-relaxed text-[var(--ink-muted)]">
          Nothing was lost. Your original files were not modified.
        </p>
      ) : null}

      {remediation ? (
        <p className="ui mt-2 text-sm leading-relaxed text-[var(--ink-muted)]">
          {remediation}
        </p>
      ) : null}

      <div className="ui mt-5 flex gap-2">
        {onRetry ? (
          <button
            onClick={onRetry}
            className="rounded-[var(--radius)] border border-[var(--rule-strong)] px-3 py-1.5 text-sm hover:bg-[var(--paper-sunken)]"
          >
            Retry
          </button>
        ) : null}
        {onDetails ? (
          <button
            onClick={onDetails}
            className="rounded-[var(--radius)] px-3 py-1.5 text-sm text-[var(--ink-muted)] hover:bg-[var(--paper-sunken)]"
          >
            Details
          </button>
        ) : null}
      </div>
    </div>
  );
}
