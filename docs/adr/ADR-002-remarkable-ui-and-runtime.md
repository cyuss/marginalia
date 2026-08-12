# ADR-002 — How reMarkFlow presents itself on a reMarkable 2

- **Status:** **OPEN — blocking Phase 1. Decision required from the project lead.**
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

## What is known and not known

**Not validated on hardware. No reMarkable has been touched.** What follows is
the state of my knowledge, offered so the decision can be made deliberately —
not as established fact. Every claim here needs a documented read-only probe
before it is relied upon.

The reMarkable 2's display is not exposed to third-party processes as a
conventional Linux framebuffer in the way earlier hardware was. Community
projects that render custom full-screen interfaces on this device generally
depend on a framebuffer shim, which works by loading a server component into
the running `xochitl` process and having client applications load a matching
client library.

That mechanism, if it is still how this works, is squarely inside invariants 1
and 2.

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

**None yet.** This ADR is deliberately unresolved.

My recommendation, strengthened since ADR-006: **pursue D as the V1 shape**,
with a hardware spike to establish whether B is achievable. D's one hole is
closed, so it is now a complete product shape rather than a partial one. D is
the only option that does not require relaxing a safety invariant, and the
invariants were written first and for good reason. If the interaction problem
in D proves unsolvable, the choice becomes an explicit, documented trade — "we
relax invariant 1 to get a real UI" — which is a decision the project lead
should make consciously, in this file, rather than one an implementation makes
by importing a library.

## Consequences

Until this is decided:

- Phase 1 (RM2 native shell) cannot start.
- `apps/remarkable` is not created.
- Capability `ExperimentalRmUi` stays `UNSUPPORTED`, feature flag OFF.
- Migration slices 1–9 in the audit report proceed regardless — they are
  valuable under every option.

## Required before this ADR can be closed

1. A documented read-only hardware probe: how a non-`xochitl` process can
   obtain display access on the target firmware, if at all.
2. ~~For option D: a design spike on the interaction mechanism.~~ **Done** —
   [ADR-006](./ADR-006-on-device-interaction.md), with the mark→intent→grant
   path exercised end to end in
   `tests/safety/tests/standalone_device.rs::the_device_can_complete_the_essential_workflow_alone`.
3. Confirmation from the project lead of which invariants, if any, may be
   relaxed — recorded here, in writing, with the reasoning.
