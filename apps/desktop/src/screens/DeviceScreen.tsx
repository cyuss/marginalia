/**
 * The Device dashboard.
 *
 * WHY this screen exists in Phase 0, before any device code: the most important
 * thing Marginalia communicates is what it will and will not do to a user's
 * hardware. That message should be on screen from the first build, not bolted
 * on once the transport works.
 */
import { Screen, ScreenHeader } from "../components/Screen";
import { EmptyState } from "../components/states";

const PERMISSIONS = [
  { label: "Read device information", allowed: true },
  { label: "Read storage", allowed: true },
  { label: "Read documents and annotations", allowed: true },
  { label: "Send a document you choose", allowed: false, note: "Phase 3" },
  { label: "Modify system software", allowed: false, note: "Never" },
];

export function DeviceScreen() {
  return (
    <Screen>
      <ScreenHeader
        title="Device"
        description="Your reMarkable stays completely stock. Marginalia adds nothing to it and changes nothing about how it works."
      />

      <EmptyState
        title="No reMarkable connected"
        description="Device detection arrives in Phase 2, and it starts read-only. Until a firmware has been tested and recorded in the compatibility matrix, Marginalia treats it as unknown — which means it will read, and refuse to write."
      />

      <section className="border-t border-[var(--rule)] pt-8">
        <h2 className="ui text-sm uppercase tracking-wide text-[var(--ink-faint)]">
          What Marginalia may do
        </h2>
        <ul className="ui mt-4 divide-y divide-[var(--rule)]">
          {PERMISSIONS.map((p) => (
            <li
              key={p.label}
              className="flex items-baseline justify-between py-3 text-sm"
            >
              <span className="flex items-baseline gap-2.5">
                <span
                  aria-hidden
                  className={
                    p.allowed
                      ? "text-[var(--accent)]"
                      : "text-[var(--ink-faint)]"
                  }
                >
                  {p.allowed ? "✓" : "✗"}
                </span>
                <span className="text-[var(--ink)]">{p.label}</span>
              </span>
              {p.note ? (
                <span className="text-[var(--ink-faint)]">{p.note}</span>
              ) : null}
            </li>
          ))}
        </ul>

        <p className="ui prose-measure mt-5 text-sm leading-relaxed text-[var(--ink-muted)]">
          Marginalia never patches xochitl, never touches the bootloader,
          kernel or system partitions, and never interferes with firmware
          updates. If you uninstall Marginalia, your reMarkable is exactly as it
          was.
        </p>
      </section>
    </Screen>
  );
}
