# ADR-005 — SQLite configuration on the device

- **Status:** Proposed
- **Date:** 2026-08-12
- **Related:** U12, Phase 1

## Context

`packages/database/src/lib.rs` currently sets, unconditionally:

```
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
PRAGMA foreign_keys = ON;
PRAGMA busy_timeout = 5000;
```

WAL was the right call for a desktop app where a background sync writes while
the UI reads. On the device, two of these assumptions need re-examining:

- **WAL** requires shared-memory support from the filesystem. Whether the
  device's storage provides it is **unknown (U12)** and must not be assumed.
- **`synchronous = NORMAL`** is a reasonable desktop trade. On a device that
  can lose power without warning while holding the user's only copy of their
  annotation history, `FULL` may be the correct choice despite the cost.

## Decision

Replace the hard-coded pragmas with an explicit `StorageProfile`:

```rust
pub enum StorageProfile {
    /// Desktop: WAL, synchronous=NORMAL.
    Workstation,
    /// Device: journal mode and durability chosen from measured evidence,
    /// defaulting to the safer option until measured.
    Device,
}
```

Until U12 is resolved on hardware, `Device` defaults to the **conservative**
combination — the one that survives power loss — rather than the fast one.
Fail closed applies to durability as much as to device writes.

## Consequences

- `marginalia_database::open` gains a profile parameter. This is migration
  slice 3 in the audit report; it is small and touches no schema.
- The characterization test `the_schema_surface_is_pinned` is unaffected: this
  changes configuration, not schema.
- A device-side test must exercise power-loss recovery in the simulator before
  the profile is considered settled.
