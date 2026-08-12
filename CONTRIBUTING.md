# Contributing to Marginalia

Thank you for considering a contribution. Marginalia touches people's devices
and their research libraries, so the bar for device-related code is high.

## Before anything else

Read [docs/safety/SAFETY_MODEL.md](./docs/safety/SAFETY_MODEL.md) and
[docs/safety/DEVICE_WRITE_POLICY.md](./docs/safety/DEVICE_WRITE_POLICY.md).
They are not background reading — they are the rules.

## Non-negotiables

1. **Never** patch `xochitl`, modify system partitions, the bootloader, the
   kernel, or firmware updates. No flag enables this.
2. Metadata sync must never transfer a file to a device.
3. Original PDFs are opened read-only. Always.
4. Unknown firmware ⇒ read-only. Fail closed, never open.
5. No automatic deletion of user data, on the device or off it.
6. Every device write goes through `SafetyManager` and carries a `WriteGrant`.

## Setup

```bash
pnpm install
pnpm dev
```

Requires Node 20+, pnpm 9+, Rust stable, and the Tauri 2 prerequisites for your
platform. **A reMarkable device is not required** — development uses the
simulator.

## Testing

```bash
pnpm test          # unit + integration
pnpm test:safety   # safety suite — never skip, never mark flaky
pnpm test:e2e
pnpm lint && pnpm typecheck
```

A PR with a failing or skipped safety test is not merged. If a safety test is
wrong, fix the test in its own PR with a written justification.

## Device work

Use the simulator (`tests/remarkable-simulator/`). Add fixtures for every new
behaviour, including the failure modes. A real device is used only for the
validation steps described in
[COMPATIBILITY_MODEL.md](./docs/remarkable/COMPATIBILITY_MODEL.md) §6, after the
simulator suite is green, and only with a device you would be willing to reset.

## Pull requests touching `packages/remarkable`

State in the description:

1. which whitelisted operation it affects (or "none — read-only");
2. its classification (GREEN / YELLOW — ORANGE needs prior discussion);
3. which safety tests cover it;
4. how rollback is tested;
5. which simulator fixtures were added.

A new device-write path that is not one of the four whitelisted operations
requires a change to the Device Write Policy **first**, reviewed on its own.

## Code style

Strict typing everywhere. Small modules. Explicit domain models. No boolean
soup — use the state machines. No magic strings — use enums. No global mutable
state. No hidden side effects, and above all no undocumented device writes.

Comments explain **why**, not what — especially around reMarkable internals,
Zotero sync, PDF geometry, safety decisions, and compatibility workarounds.

## Uncertainty

If you do not know how a reMarkable behaves, **do not guess**. Mark the
capability `UNKNOWN`, add an entry to
[OPEN_QUESTIONS.md](./docs/development/OPEN_QUESTIONS.md), and stop. Never
compensate for uncertainty with a more invasive approach.

## Architectural decisions

Significant decisions get an ADR in `docs/architecture/`, numbered, with context,
options, decision, and consequences.
