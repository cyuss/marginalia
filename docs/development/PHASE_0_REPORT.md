# Phase 0 — completion report

Date: 2026-08-12
Status: **implemented**

---

## What was built

| Area | Crate / path | Notes |
|---|---|---|
| Domain model | `packages/core` | Entities, both state machines, typed errors. Depends on nothing. |
| Safety layer | `packages/safety` | `SafetyManager`, `WriteGrant`, classification, snapshots, feature flags |
| Logging | `packages/observability` | Structured logs, `SAFETY` audit channel, `Redacted<T>` |
| Storage | `packages/database` | SQLite (WAL, FKs), versioned migrations, repositories |
| Device boundary | `packages/remarkable` | `DeviceProvider` port, capability resolver, compatibility matrix |
| Simulator | `tests/remarkable-simulator` | Profiles, deterministic fault injection, invariant assertions |
| Safety suite | `tests/safety` | The mandatory tests from `SAFETY_MODEL.md` §9 |
| App shell | `apps/desktop` | Tauri 2 + React, navigation, design tokens, all three UI states |
| CI | `.github/workflows/ci.yml` | fmt, clippy, tests, safety suite, typecheck, desktop build |

## The three guards on INV-2, implemented

The promise that a metadata sync never transfers a PDF is enforced three
independent ways, so no single mistake can undo it:

1. **Type level** — `MetadataOperation` has no variant that can express a file
   transfer; `SyncExecutor` accepts only that type
   (`packages/core/src/sync.rs`).
2. **State machine** — exactly one edge reaches `TransferPending`, and only via
   `UserRequestedSend`. A test enumerates every (state, event) pair and asserts
   no other route exists (`packages/core/src/document.rs`).
3. **Schema** — a `CHECK` constraint rejects a `TRANSFER` job whose trigger is
   not `USER` (`packages/database/migrations/0001_initial.sql`).

## The write grant, implemented

`WriteGrant` holds a field of the private type `Seal`. Rust forbids
constructing it outside `packages/safety`, so a device write that skipped
authorisation does not compile. Grants are single-use, operation-scoped,
device-scoped, document-scoped and TTL-bounded.

## Bug found and fixed during implementation

`SafetyManager::check_preconditions` initially used `pre.storage?` in a function
returning `Option<DenialReason>`. On unknown storage that returns `None` —
meaning *no denial* — a **fail-open** in the exact code path meant to fail
closed. Replaced with an explicit `StorageUnknown` denial, and covered by
`s2_unknown_storage_denies_rather_than_assuming_room`.

## Deliberately not built

- **No device transport.** No USB, SSH or HTTP code exists. Blocked on
  `OPEN_QUESTIONS.md` U1/U2 and, correctly, on Phase 2.
- **No Zotero adapter.** Phase 1.
- **No PDF engine.** Phase 4. `pdfium-render` is chosen (ADR-001) but not wired.
- **No mock data in the UI.** Screens show honest empty states.

## Shipped state of the compatibility matrix

Every capability is `UNKNOWN` or `UNSUPPORTED`, and every feature flag is OFF.
Marginalia can currently do nothing to a device — which is the correct state
for a project that has never been run against one. A test
(`the_bundled_matrix_grants_no_writes_yet`) fails if that changes without a
validation report.

## Toolchain note

`rust-toolchain.toml` pins the project to Rust 1.85. Tauri 2 requires 1.77+,
and the modern dependency tree (`syn`, `indexmap`) requires 1.71+. The core
workspace excludes `apps/desktop/src-tauri`, so the domain, safety and database
layers build and test without the Tauri toolchain.

## Verification

Run on 2026-08-12, Rust 1.85.0, macOS arm64:

```
cargo test --workspace      118 passed, 0 failed, 2 ignored
cargo test -p marginalia-safety-suite
                             27 passed, 0 failed, 2 ignored
cargo clippy --workspace --all-targets -- -D warnings    clean
cargo fmt --all -- --check                               clean
```

The two ignored tests are S6 and S11, which need the PDF engine (Phase 4) and
are named `#[ignore]` stubs rather than silent gaps.

The central claim is verified by a `compile_fail` doctest in
`packages/safety/src/lib.rs`: constructing a `WriteGrant` outside the safety
crate does not compile.

**Verified 2026-08-12**, after Node 20 was installed on the development machine:

```
pnpm typecheck                              clean (tsc --noEmit, strict)
vite build     87 modules · 177 kB · 56 kB gzipped
```

The Tauri shell itself still has not been launched — that needs a windowing
session — but the TypeScript compiles under strict settings and the bundle
builds.

## Phase 0 exit criteria

- [x] Monorepo skeleton, both workspaces wired
- [x] Desktop shell with navigation, empty/loading/error states, design tokens
- [x] Domain models and both state machines
- [x] SQLite with migrations, WAL, FK enforcement, safety constraints
- [x] `SafetyManager` + `WriteGrant` + classification + snapshots
- [x] `FeatureFlagManager`, everything OFF
- [x] Structured logging with a `SAFETY` channel and secret redaction
- [x] Test infrastructure and the simulator
- [x] Safety suite covering S1–S5, S7–S10, S12–S15
- [x] CI
- [x] A device write without a grant does not compile
- [ ] S6 and S11 — require the PDF engine (Phase 4), present as named
      `#[ignore]` stubs so the gap is visible

## Next

Phase 1 (Zotero metadata) is unblocked and needs no device. Its first task is
resolving `OPEN_QUESTIONS.md` U4 against a real library — read-only, therefore
safe to do immediately.
