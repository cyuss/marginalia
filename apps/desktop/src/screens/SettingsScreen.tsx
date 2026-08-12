import { Screen, ScreenHeader } from "../components/Screen";

/**
 * Settings.
 *
 * The feature-flag list is shown read-only in Phase 0. Every flag is OFF, and
 * the screen says so plainly rather than hiding an empty section.
 */
const FLAGS = [
  { name: "Send to reMarkable", state: "Off — arrives in Phase 3" },
  { name: "Native PDF annotations", state: "Off — experimental" },
  { name: "Two-way tag sync", state: "Off — arrives in Phase 8" },
  { name: "reMarkable companion", state: "Off — experimental" },
];

export function SettingsScreen() {
  return (
    <Screen>
      <ScreenHeader
        title="Settings"
        description="Marginalia stores everything locally. There is no account, no server, and no telemetry."
      />

      <section className="pt-8">
        <h2 className="ui text-sm uppercase tracking-wide text-[var(--ink-faint)]">
          Features
        </h2>
        <ul className="ui mt-4 divide-y divide-[var(--rule)]">
          {FLAGS.map((flag) => (
            <li
              key={flag.name}
              className="flex items-baseline justify-between py-3 text-sm"
            >
              <span className="text-[var(--ink)]">{flag.name}</span>
              <span className="text-[var(--ink-faint)]">{flag.state}</span>
            </li>
          ))}
        </ul>
        <p className="ui prose-measure mt-4 text-sm text-[var(--ink-muted)]">
          Experimental features are always off by default, and can never enable
          anything that modifies your reMarkable&rsquo;s system software.
        </p>
      </section>
    </Screen>
  );
}
