# Phase 0 → standalone reMarkable 2: audit and migration report

Status: **Phase 0.5, first deliverable — audit complete, decision required**
Date: 2026-08-12
Author: implementation session

> **Read §6 first if you read nothing else.** The audit surfaced one finding
> that the roadmap's own rules say must stop work and request a decision: the
> product decision ("a native E-Ink UI running on the device") and safety
> invariants 1–4 ("never inject into `xochitl`, never replace system libraries,
> never install Toltec automatically") may be mutually unsatisfiable on a
> reMarkable 2. This must be resolved before Phase 1.

---

## 1. Baseline evidence

Commands run unchanged, on the repository as it stood before this session's
changes. Rust 1.85.0, macOS arm64.

| Command | Result |
|---|---|
| `cargo test --workspace` | **118 passed, 0 failed, 2 ignored** |
| `cargo test -p marginalia-safety-suite` | 27 passed, 0 failed, 2 ignored |
| `cargo clippy --workspace --all-targets -- -D warnings` | clean |
| `cargo fmt --all -- --check` | clean |
| `cargo test --workspace --doc` | 1 passed (the `compile_fail` grant proof) |

The two ignored tests are S6 and S11, which need the PDF engine (Phase 4).

Schema: **version 1**, 18 tables + 1 view + 1 trigger.
Document state machine: **20 legal edges** out of 120 (state, event) pairs.
Device operations: **23**, classified 9 GREEN / 4 YELLOW / 2 ORANGE / 8 RED.
Feature flags: **8**, all OFF.

> **Resolved 2026-08-12.** The directory was not a git repository when the audit
> ran. It now is: initial commit `cf56d63`, tagged `phase-0.5-audit`. A
> `phase-0-baseline` tag pointing at a Phase-0-only tree is not achievable
> retroactively, because the repository was created after the audit artifacts
> already existed; the Phase 0 evidence is recorded in
> [PHASE_0_REPORT.md](../development/PHASE_0_REPORT.md) instead. From here every
> migration slice gets its own commit, which is what the incremental strategy
> actually needs.

## 2. What the audit actually found

The headline is better than expected.

**The Phase 0 core is already portable.** It was written against the ports-and-
adapters discipline, and the discipline held:

| Crate | Workspace deps | External deps | Cross-compiles to `armv7-unknown-linux-gnueabihf` |
|---|---|---|---|
| `marginalia-core` | **none** | serde, serde_json, thiserror, ulid, chrono, sha2 | ✅ verified |
| `marginalia-safety` | core | serde, serde_json, thiserror, chrono, tracing | ✅ verified |
| `marginalia-observability` | none | serde, serde_json, chrono, tracing, tracing-subscriber | ✅ verified |
| `marginalia-remarkable` | core, safety | serde, thiserror, chrono, toml, tracing | ✅ verified |
| `marginalia-database` | core | rusqlite (bundled), + above | ⚠ blocked on cross-C toolchain, not on architecture |

Verified by:

```bash
rustup target add armv7-unknown-linux-gnueabihf
cargo check --target armv7-unknown-linux-gnueabihf \
  -p marginalia-core -p marginalia-safety \
  -p marginalia-observability -p marginalia-remarkable
# Finished `dev` profile in 6.99s
```

`armv7-unknown-linux-gnueabihf` is the reMarkable 2's target (32-bit ARMv7,
i.MX7 class). **This is a real result, not a plan**: the domain model, the
entire safety layer, the capability/compatibility layer and the logging layer
compile for the device today, unmodified.

**There is no desktop coupling to remove from the portable crates.** A scan for
`tauri`, `react`, `node`, `napi`, `wry`, `webview`, `keyring`, `ssh2`,
`reqwest` across all five portable crates returns **zero non-comment hits**.
Tauri appears only in `apps/desktop/src-tauri`, which is already a separate
Cargo workspace.

The consequence for the roadmap: **Phase 0.5's "extract a portable core"
work is largely already done.** What remains is smaller and more specific than
the roadmap assumes — see §4.

## 3. Module-by-module inventory

| Module | Verdict | Notes |
|---|---|---|
| `packages/core/ids.rs` | **preserve** | pure; ULID, no I/O |
| `packages/core/checksum.rs` | **preserve** | pure SHA-256 |
| `packages/core/document.rs` | **preserve** | state machine, characterized |
| `packages/core/sync.rs` | **preserve** | the `MetadataOperation` firewall — do not touch |
| `packages/core/intent.rs` | **preserve** | `ExplicitUserIntent` |
| `packages/core/device.rs` | **preserve** | firmware/capability/storage value types |
| `packages/core/annotation.rs` | **preserve** | geometry is target-neutral |
| `packages/core/zotero.rs` | **preserve** | mirrors, no transport |
| `packages/core/tag.rs` | **preserve** | normalisation is pure |
| `packages/core/error.rs` | **preserve** | typed errors |
| `packages/safety/*` | **preserve** | grants, classification, flags, snapshots, manager |
| `packages/observability/*` | **preserve** | ⚠ `tracing-subscriber` + JSON is heavier than the device needs; measure before shipping (see U14) |
| `packages/database/migrations` | **preserve** | schema is target-neutral |
| `packages/database/lib.rs` | **refactor (small)** | WAL is hard-coded; RM2 filesystem may need a different journal mode. Move the pragma choice behind a `StorageProfile` parameter. |
| `packages/database/repositories.rs` | **preserve** | plain SQL |
| `packages/remarkable/provider.rs` | **rename + keep** | this is the *host-side device transport* port. On a standalone device it becomes the wrong name — the device does not "provide" itself. Split into `DeviceIntrospection` (what the app running on the device asks about itself) and `RemoteDeviceTransport` (desktop companion only). |
| `packages/remarkable/capability.rs` | **preserve** | resolution logic is target-neutral |
| `packages/remarkable/compatibility.rs` | **preserve** | matrix loader |
| `tests/remarkable-simulator` | **extend** | simulates a device *from the outside*. The standalone runtime needs an additional in-process simulator for filesystem, storage pressure, process kill, clock skew, network. |
| `tests/safety` | **preserve** | S1–S15 |
| `apps/desktop/*` | **preserve, demote** | becomes the optional companion; no code change required yet |
| `packages/ui`, `packages/shared-types`, `features/*` | **retire from the plan** | declared in `pnpm-workspace.yaml` but **never created**. The workspace globs currently match nothing. Either create them or drop the globs. |

### An honest correction to the roadmap's assumptions

The roadmap's target tree proposes renaming `packages/` → `crates/` and adding
`crates/application/`. Two observations:

1. **`packages/` → `crates/` buys nothing today** and would touch every
   manifest, every import path in tests, the CI workflow, and both architecture
   test files. The roadmap itself says "do not rename or reorganize unrelated
   code". I have **not** done this rename and recommend against it unless the
   pnpm/Cargo naming collision becomes a genuine problem.
2. **`application/` does not exist yet and should not be created speculatively.**
   The roadmap's own migration rule 4 says "create ports at real seams… avoid
   speculative abstractions". There is currently exactly one use case
   implemented end-to-end (authorize-a-write). The application layer should be
   extracted when the second consumer exists, not before.

## 4. What actually needs to move, and in what order

Each slice is independently reversible and leaves every test green.

| # | Slice | Size | Risk |
|---|---|---|---|
| 1 | ~~`git init`, commit baseline, tag~~ **done** | trivial | none |
| 2 | ~~Architecture + characterization tests~~ **done** | done | none |
| 3 | ~~`StorageProfile` parameter instead of hard-coded WAL~~ **done** | small | low |
| 4 | ~~Split `DeviceProvider`~~ **done** — `DeviceIntrospection` + `RemoteDeviceTransport` | small | low |
| 5 | ~~`CredentialStore` port~~ **done** (port only; impls deferred to their consumer) | small | medium |
| 6 | ~~`Clock` port~~ **done** | small | low |
| 7 | ~~Cross-compilation CI job~~ **done** | small | low |
| 8 | Cross-C toolchain for `marginalia-database` — **blocked: no Docker or `cross` available here** | medium | low |
| 9 | ~~Simulator device-side faults~~ **done** — 10 tests | medium | low |
| 10 | Minimal `apps/remarkable` smoke app | medium | **gated on §6** |

Slices 1–9 were worth doing under **every** outcome of the §6 decision; all but
slice 8 are now complete. Slice 10 remains gated.

## 5. Database compatibility

The schema is target-neutral and needs no migration to run on the device. Two
device-specific concerns:

- **Journal mode.** `packages/database/src/lib.rs` sets `journal_mode = WAL`
  unconditionally. WAL requires shared-memory support from the filesystem; on
  the device's storage this needs verification, not assumption. Slice 3 makes
  this a parameter. Until measured, it is **U12**.
- **Free-space reserve.** The reserve currently guards *device document
  transfers* from the desktop. In the standalone model the same database, index
  and downloaded PDFs all live on the device, so the reserve must also guard
  the app's own writes. This is a Phase 1 design change, not a Phase 0.5 one.

No forward migration is required. No data would be lost by an extraction,
because no user data exists yet.

## 6. ⚠ BLOCKING: how does a third-party app draw to a reMarkable 2 screen?

This is the finding that stops Phase 1.

The product decision requires "a small touch/stylus-friendly E-Ink UI" running
on the device. Safety invariants 1–4 forbid patching or injecting into
`xochitl`, replacing system libraries, and installing Toltec automatically.

**Every route I am aware of for putting a third-party full-screen UI on a
reMarkable 2 appears to require one of the following**, and each collides with
a stated invariant:

| Route | What it involves | Collides with |
|---|---|---|
| **A. Framebuffer shim** (the common community approach) | A server component loaded into `xochitl`'s process via `LD_PRELOAD`, with client apps preloading a matching client library | **Invariant 1** (inject into `xochitl`), **2** (system libraries) |
| **B. Stop `xochitl`, drive the display directly** | `systemctl stop xochitl` while the app runs; app talks to the EPD controller itself | **Invariant 3** in spirit (replaces the native workflow while running); needs undocumented display access. Does *not* patch files. |
| **C. Launcher ecosystem** | Install a third-party launcher, generally distributed via a package manager | **Invariant 4/9** (Toltec, system-level package management) |

**I have not validated any of this on hardware, and I am not asserting it as
fact.** It is recorded as **U11 — the highest-risk unknown in the plan**. The
roadmap's own rule applies: unknown means `UNKNOWN`, and I must not compensate
for uncertainty by picking the more invasive option.

### The fourth option the roadmap has not considered

**D. No custom UI on the device at all.** reMarkFlow runs on the device as a
headless, bounded, user-startable agent. The *native reMarkable reader is the
UI*:

- Zotero metadata syncs on the device into the local database;
- the library browser is a **generated document** — reMarkFlow writes a small
  PDF or notebook index into its own document area, which the user opens and
  reads in the native reader;
- "Download this PDF" is triggered by a mechanism that needs design (a
  generated action document, a tag, a companion tap) — this is the weak point
  and needs its own spike;
- annotations are read back, extracted, indexed, and exported to Zotero;
- everything else — Inbox, search results, note bundles — is delivered as
  generated documents the native reader already renders beautifully.

Option D satisfies **all fourteen invariants as written**, requires no display
access, no `xochitl` interaction, no launcher, and no system package manager.
It is a genuinely different product shape — worse for interactivity, far better
for safety — and it is the only option I can currently see that does not
require relaxing an invariant.

### Decision taken

**Gather evidence before choosing.** A read-only hardware spike protocol is
written and ready to run:
[`../remarkable/DISPLAY_ACCESS_SPIKE.md`](../remarkable/DISPLAY_ACCESS_SPIKE.md).
It writes nothing to the device, stops no service, installs nothing, and
resolves U11 plus — opportunistically — U12, U16 and the first real resource
baseline. ADR-002 stays open until the evidence is in.

Phase 1 cannot start until then. Slices 2–9 in §4 proceed regardless.

## 7. Unknowns

Legacy unknowns U1–U10 are unchanged and recorded in
[`../development/OPEN_QUESTIONS.md`](../development/OPEN_QUESTIONS.md). New:

| ID | Unknown | Blocks | Severity |
|---|---|---|---|
| **U11** | How a third-party app draws to the RM2 screen without violating invariants 1–4 | **Phase 1, all UI work** | **blocking** |
| U12 | Whether SQLite WAL is safe/performant on the device's filesystem | Phase 1 storage | high |
| U13 | Whether an app can be launched and persist across firmware updates using only manifest-owned files | Phase 1 packaging | high |
| U14 | Runtime cost of `tracing-subscriber` + JSON logging on the device | Phase 1 budgets | medium |
| U15 | Whether a PDF text/geometry extractor exists that runs within RM2 budgets (PDFium has no obvious prebuilt armv7 binary; ADR-001 assumed a desktop target) | Phase 5 | high |
| U16 | TLS on the device: available root store, and whether to bundle one | Phase 2 | medium |
| U17 | Cross-compilation of `libsqlite3-sys` — needs a cross-C toolchain or a Docker/`cross` build | Slice 8 | low |

**None of the RM2 resource budgets required by the roadmap have been measured.**
Binary size, idle/peak RAM, startup time, CPU, battery, index growth, E-Ink
refresh behaviour: all unmeasured, because nothing has run on a device. The
roadmap requires these in Phase 0.5; they cannot be produced without either
hardware access or a decision on §6.

## 8. What this session did **not** do

Deliberately, per the execution brief:

- did not rename `packages/` → `crates/`;
- did not create an `application/` layer with no second consumer;
- did not create `apps/remarkable`;
- did not touch a real device;
- did not add Zotero, PDF-download, or annotation functionality;
- did not modify the desktop app;
- did not delete or weaken any Phase 0 infrastructure;
- did not change any safety type, classification, or test.

Net change to production code this session: **zero**. Everything added is a
test or a document.

## 9. Artifacts produced

| Artifact | Path |
|---|---|
| This report | `docs/migration/phase-0-to-standalone-rm2.md` |
| Architecture tests (6) | `tests/architecture/tests/boundaries.rs` |
| Characterization tests (13) | `tests/characterization/tests/phase0_behavior.rs` |
| ADR-002 — RM2 UI and runtime (**OPEN**) | `docs/adr/ADR-002-remarkable-ui-and-runtime.md` |
| ADR-003 — cross-compilation target | `docs/adr/ADR-003-cross-compilation.md` |
| ADR-004 — on-device credential storage | `docs/adr/ADR-004-device-credentials.md` |
| ADR-005 — on-device storage profile | `docs/adr/ADR-005-device-storage-profile.md` |

Still owed, and blocked on §6: packaging/launcher ADR, process-lifecycle ADR,
measured budget report, dependency graph diagram export.
