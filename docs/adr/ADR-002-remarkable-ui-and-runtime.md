# ADR-002 — How reMarkFlow presents itself on a reMarkable 2

- **Status:** **Accepted — option D.** Decided 2026-08-12 on the evidence in
  [`ECOSYSTEM.md`](../remarkable/ECOSYSTEM.md), not on preference.
- **Date:** 2026-08-12
- **Supersedes:** nothing
- **Related:** invariants 1–4, 9; unknown U11

---

## Context

The standalone product decision requires the essential reading workflow to run
on the reMarkable 2 itself, with "a small touch/stylus-friendly E-Ink UI with
explicit refresh policy" (Phase 1).

The safety invariants require, without exception:

1. never patch, replace, inject into, or depend on private modifications to
   `xochitl`;
2. never patch the kernel, bootloader, boot partition, recovery path, firmware
   updater, or system libraries/files;
3. do not replace the home screen, PDF reader, notebook app, or native
   workflow;
4. install only isolated reMarkFlow-owned files in a documented area;
9. never install a system package manager automatically.

These two requirements are in tension, and the tension is not obviously
resolvable. This ADR exists to make the choice explicit rather than to let an
implementation quietly pick one.

## What is known

Desk research on 2026-08-12 replaced the guesswork this ADR originally
contained. The findings, with sources, are in
[`ECOSYSTEM.md`](../remarkable/ECOSYSTEM.md). In short:

- reMarkable's own documentation states there is **no official or supported
  mechanism** for third-party applications to run on the device or reach the
  display, and that `xochitl` is proprietary with no source available.
- The community solution, **rm2fb**, works by `LD_PRELOAD`ing a server into a
  system binary and `LD_PRELOAD`ing a client shim into each application, which
  then intercepts `/dev/fb0`.

That mechanism is squarely inside invariants 1, 2 and 4. It is careful work
solving a problem reMarkable declined to solve — and it is incompatible with
what this project promised.

**Still not verified on hardware.** These are vendor and community sources read
from the outside. But they point the same way from every direction examined.

## Options

### A — Framebuffer shim (community-standard approach)

Load a shim into `xochitl`; render through it.

- ➕ It is the approach with the most existing art, and gives a real
  interactive UI.
- ➖ Requires **relaxing invariants 1 and 2**. The shim is a private
  modification to a running system process.
- ➖ Couples reMarkFlow's viability to an undocumented internal interface that a
  firmware update can change without notice — the exact failure mode the
  capability layer exists to prevent.
- ➖ A crash in the shim is a crash in the user's reading application.

### B — Stop `xochitl`, take the display, restart it on exit

- ➕ Patches nothing. Installs nothing into a system path. Fully reversible by
  restarting a service.
- ➕ Arguably compatible with invariants 1, 2 and 4 as literally written.
- ➖ Violates invariant 3 in spirit while running: the native workflow is
  unavailable, and the user's reading application is being stopped by us.
- ➖ Still requires display access whose availability is unknown.
- ➖ Failure mode is severe: crash while `xochitl` is stopped leaves the user
  looking at a dead screen until reboot.

### C — Third-party launcher ecosystem

- ➕ Solves lifecycle, display and app-switching in one step.
- ➖ Distribution is normally via a system package manager, colliding with
  invariants 4 and 9.
- ➖ Inherits every compatibility risk of the launcher.

### D — No custom UI: a headless agent, with the native reader as the interface

reMarkFlow runs on the device as a bounded, user-startable background service
and never draws to the screen. Everything the user sees is a **document the
native reader already knows how to display**:

```
Zotero  ──sync──►  reMarkFlow agent  ──generates──►  "Library.pdf"
                          │                          "Annotation Inbox.pdf"
                          │                          "Search results.pdf"
                          ▼
                   local SQLite + index          the user opens these
                                                 in the native reader
```

- ➕ **Satisfies all fourteen invariants exactly as written.** No display
  access, no `xochitl` interaction, no launcher, no package manager, no system
  files.
- ➕ The native reader is genuinely excellent at rendering documents; we would
  be reusing the best part of the device rather than competing with it, which
  is the stated product philosophy.
- ➕ Failure mode is benign: the agent dies, the user keeps reading.
- ➕ Lowest resource cost — no rendering, no input handling, no refresh policy.
- ➕ **The interaction problem is now solved.** It was this option's real cost.
  [ADR-006](./ADR-006-on-device-interaction.md) proposes the *request form*: the
  agent generates an index with a tick box beside each entry, the user marks one
  with the stylus, and the agent reads the annotation layer on its next wake.
  That needs only annotation *reading* — the GREEN capability Phase 5 requires
  anyway. The derivation is implemented and tested in
  `marginalia_core::request_form`.
- ➖ Latency: a generated index refreshes on a cycle, not on a tap.
- ➖ It is a different product from the one the roadmap describes.

## Decision

**Option D.** No custom UI on the device; the native reader is the interface,
and generated documents are what the user sees.

This is not the cautious choice among several. A and C require injecting a
library into a system process, and B requires display access that no documented
route provides without the same injection. **Every alternative is unavailable
to a project holding invariants 1, 2 and 4** — so the decision is what remains,
not what was preferred.

Option D's one real weakness, having no way for the user to express a
per-document request, was closed by [ADR-006](./ADR-006-on-device-interaction.md):
a tick box on a generated form, read back from the annotation layer. That needs
only annotation *reading*, which is GREEN.

If a future firmware or a supported reMarkable API changes the display picture,
this decision should be revisited on its merits. Relaxing an invariant to get a
richer UI remains possible, but it would be an explicit, written trade recorded
here — not something an implementation arrives at by importing a library.

## Consequences

- **Phase 1 is unblocked**, in the option D shape: a headless agent, not a
  native shell. `apps/remarkable` exists and is that agent.
- Capability `ExperimentalRmUi` stays `UNSUPPORTED` permanently, and its
  feature flag never ships on. It is now unsupported by decision rather than by
  ignorance.
- The roadmap's Phase 1 wording ("a small touch/stylus-friendly E-Ink UI") is
  superseded. The interface is documents; the gesture is a stylus mark.
- Latency becomes a product property rather than a bug: a request is seen at
  the next sync. The generated documents must say so.
- **Phase 5 needs re-scoping.** The same research found that firmware 3.x
  changed annotation storage and that community tooling has not caught up —
  quotable highlight text may not be recoverable at all. See
  [`ECOSYSTEM.md`](../remarkable/ECOSYSTEM.md) §3 and U3.

## Still required, but no longer blocking

A hardware probe remains the only way to move any capability off `UNKNOWN`.
[`DISPLAY_ACCESS_SPIKE.md`](../remarkable/DISPLAY_ACCESS_SPIKE.md) is written
and ready; its display section is now confirmatory rather than exploratory, and
its storage, TLS and resource sections are still the cheapest experiments
available.
