# ADR-003 — Cross-compilation target and build strategy for the reMarkable 2

- **Status:** Accepted (target), Proposed (toolchain)
- **Date:** 2026-08-12
- **Related:** U17

## Context

The standalone runtime must produce an ARM binary for the reMarkable 2 without
requiring a device, and reproducibly in CI.

## Decision

**Target triple: `armv7-unknown-linux-gnueabihf`** — 32-bit ARMv7 hard-float,
matching the device's i.MX7-class SoC.

This is not a guess. It was verified this session:

```bash
rustup target add armv7-unknown-linux-gnueabihf
cargo check --target armv7-unknown-linux-gnueabihf \
  -p marginalia-core -p marginalia-safety \
  -p marginalia-observability -p marginalia-remarkable
# Finished `dev` profile in 6.99s
```

All four pure-Rust portable crates compile for the device today, unmodified.

## The one blocker

`marginalia-database` fails to cross-compile because `libsqlite3-sys` with the
`bundled` feature compiles SQLite from C, and no cross C toolchain is present:

```
error: failed to run custom build command for `libsqlite3-sys v0.27.0`
```

This is a toolchain gap, not an architecture problem. Options:

| Option | Notes |
|---|---|
| **`cross` + Docker** (recommended) | Reproducible, CI-friendly, no host toolchain to document per developer |
| Host cross-gcc (`arm-linux-gnueabihf-gcc`) | Fast locally, but each developer installs it differently per OS |
| Link the device's own libsqlite3 | Removes the C build, but couples us to a system library version — and the roadmap forbids depending on system libraries |

Recommendation: `cross`, with the host toolchain documented as an optional
fast path. The third option should be rejected: linking against a system
library is exactly the coupling the compatibility layer exists to avoid.

## Consequences

- CI gains a job that cross-checks the portable crates on every pull request —
  cheap, and it catches a non-portable dependency the day it is added.
- A release build additionally needs `--release`, size optimisation, and a
  measured binary-size budget (unmeasured; see the audit report §7).
- `pdfium` remains an open problem for Phase 5: ADR-001 chose it for a desktop
  target, and there is no obvious prebuilt armv7 binary. Recorded as U15.
