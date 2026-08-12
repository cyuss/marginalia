/**
 * Primary navigation.
 *
 * The Safe Mode badge at the bottom is deliberate and permanent: the user
 * should be able to tell, at a glance and at all times, what Marginalia is
 * currently allowed to do to their device.
 */

export type ScreenId =
  | "library"
  | "inbox"
  | "zotero"
  | "device"
  | "activity"
  | "settings";

const GROUPS: { items: { id: ScreenId; label: string }[] }[] = [
  { items: [{ id: "library", label: "Library" }] },
  { items: [{ id: "inbox", label: "Annotation Inbox" }] },
  {
    items: [
      { id: "zotero", label: "Zotero" },
      { id: "device", label: "Device" },
      { id: "activity", label: "Activity" },
    ],
  },
];

export function Sidebar({
  current,
  onNavigate,
}: {
  current: ScreenId;
  onNavigate: (id: ScreenId) => void;
}) {
  return (
    <nav className="ui flex w-56 shrink-0 flex-col border-r border-[var(--rule)] bg-[var(--paper-sunken)] px-3 py-6">
      <div className="px-3 pb-8">
        <span className="text-[15px] tracking-tight text-[var(--ink)]">
          Marginalia
        </span>
      </div>

      <div className="flex-1 space-y-6">
        {GROUPS.map((group, i) => (
          <div key={i} className="space-y-0.5">
            {group.items.map((item) => (
              <NavItem
                key={item.id}
                label={item.label}
                active={current === item.id}
                onClick={() => onNavigate(item.id)}
              />
            ))}
          </div>
        ))}
      </div>

      <div className="space-y-3 border-t border-[var(--rule)] pt-4">
        <NavItem
          label="Settings"
          active={current === "settings"}
          onClick={() => onNavigate("settings")}
        />
        <SafeModeBadge />
      </div>
    </nav>
  );
}

function NavItem({
  label,
  active,
  onClick,
}: {
  label: string;
  active: boolean;
  onClick: () => void;
}) {
  return (
    <button
      onClick={onClick}
      aria-current={active ? "page" : undefined}
      className={[
        "w-full rounded-[var(--radius)] px-3 py-1.5 text-left text-sm transition-colors",
        active
          ? "bg-[var(--paper-raised)] text-[var(--ink)]"
          : "text-[var(--ink-muted)] hover:bg-[var(--paper-raised)] hover:text-[var(--ink)]",
      ].join(" ")}
    >
      {label}
    </button>
  );
}

function SafeModeBadge() {
  return (
    <div className="px-3 pb-1">
      <div className="flex items-center gap-2">
        <span
          aria-hidden
          className="h-1.5 w-1.5 rounded-full bg-[var(--accent)]"
        />
        <span className="text-xs text-[var(--ink-muted)]">Safe Mode</span>
      </div>
      <p className="mt-1 text-[11px] leading-snug text-[var(--ink-faint)]">
        Read-only until a device is validated
      </p>
    </div>
  );
}
