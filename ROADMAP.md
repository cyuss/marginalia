# Marginalia / reMarkFlow — Roadmap

Status: **Draft v2 — Phase 0 complete; standalone-RM2 adaptation in progress**
Primary target: **reMarkable 2**, as the primary autonomous runtime
Repository model: one pnpm + Cargo monorepo

> **Naming is unresolved.** The code, crates, database and documentation all say
> **Marginalia**, per an explicit instruction. The v2 roadmap heading says
> **reMarkFlow (working name)**. Nothing has been renamed; a rename would touch
> every manifest, import path, table comment and document, and is a decision
> worth making once. See the open item at the end of this file.

## Product decision (v2)

The essential reading workflow must run **on the reMarkable 2 itself**, without
requiring a Mac/PC, a permanent local process, or a Marginalia server. The RM2
application owns its local database, search index, cached Zotero metadata,
downloaded PDFs, annotations, notes, tags, journal and recovery state. Internet
is needed only for operations that inherently contact Zotero.

The desktop application stays in the same monorepo and becomes an **optional
companion and power mode**. No essential RM2 workflow may depend on it.

> ⚠ **Phase 1 is blocked.** The Phase 0.5 audit found that the on-device UI
> requirement may be unsatisfiable alongside safety invariants 1–4. See
> [ADR-002](docs/adr/ADR-002-remarkable-ui-and-runtime.md) and
> [the audit report](docs/migration/phase-0-to-standalone-rm2.md) §6.
> A decision is required before Phase 1 work begins. **Chosen route: gather
> evidence first** — see the read-only
> [hardware spike protocol](docs/remarkable/DISPLAY_ACCESS_SPIKE.md).

Phases are sequential. A phase does not start while a previous phase has failing
tests or unresolved safety regressions. Phase 0 is the validated baseline and is
not to be rewritten wholesale.

Every phase ends with: unit and integration tests; lint and typecheck;
architecture/dependency tests; the complete safety suite; simulator and
fault-injection tests; documentation, ADRs, capability matrix and this roadmap
updated; and a review proving desktop is not a hidden runtime dependency of an
RM2-essential feature.

Legend: ☐ not started · ◐ in progress · ☑ done · ⚠ blocked/unknown

## Non-negotiable safety invariants

These apply to every phase and cannot be relaxed by a feature flag:

1. Never patch, replace, inject into, or depend on private modifications to `xochitl`.
2. Never patch the kernel, bootloader, boot partition, recovery path, firmware updater, or system libraries/files.
3. Do not replace the home screen, PDF reader, notebook app, or native reMarkable workflow.
4. Install only isolated Marginalia-owned files in a documented application/data area, with a complete manifest.
5. Uninstall removes only manifest-owned files and leaves native documents and system state untouched.
6. Unknown firmware or capability means `UNKNOWN`, never implicitly `SUPPORTED`.
7. All writes require a typed `WriteGrant`, an `ExplicitUserIntent`, preflight validation, a verified `SafetySnapshot`, checksums, post-verification, and a tested rollback path.
8. Metadata synchronization must be unable to transfer PDF bytes.
9. PDF download is a distinct command for one explicitly selected attachment after confirmation. No bulk sync, no prefetch.
10. Originals are immutable. Derived artefacts are stored separately with provenance.
11. Experimental features default OFF; flags cannot bypass grants, gates, snapshots, checksums, or rollback.
12. The simulator and fault suite must pass before the first real-device write for a phase.
13. Logs never contain API keys, authorization headers, document contents, or note text by default.
14. Outage, crash, low storage, interrupted download, or power loss must leave the app recoverable and native RM2 content usable.

---

## Phase 0 — Foundation ☑

The validated baseline. See
[PHASE_0_REPORT.md](docs/development/PHASE_0_REPORT.md).

Baseline evidence recorded 2026-08-12 (Rust 1.85.0): `cargo test --workspace`
118 passed / 0 failed / 2 ignored; clippy clean with `-D warnings`; fmt clean.
Schema version 1, 18 tables. 20 state-machine edges. 23 device operations
(9 GREEN / 4 YELLOW / 2 ORANGE / 8 RED). 8 feature flags, all OFF.

## Phase 0.5 — Audit, preserve, and extract the portable RM2 foundation ◐

### Audit ☑

- ☑ ran and recorded all existing validation commands unchanged
- ☑ inventoried workspaces, crates, public APIs, schema, flags, state machines, safety classifications, UI coupling
- ☑ mapped every dependency edge; **found zero desktop coupling in the portable crates**
- ☑ **verified `core`, `safety`, `observability`, `remarkable` cross-compile to `armv7-unknown-linux-gnueabihf`**
- ☑ produced [the audit and migration report](docs/migration/phase-0-to-standalone-rm2.md) with a preserve/move/refactor/retire inventory
- ☑ ADR drafts: [002 RM2 UI](docs/adr/ADR-002-remarkable-ui-and-runtime.md) (**open/blocking**), [003 cross-compilation](docs/adr/ADR-003-cross-compilation.md), [004 device credentials](docs/adr/ADR-004-device-credentials.md), [005 storage profile](docs/adr/ADR-005-device-storage-profile.md)
- ☑ raised unknowns U11–U17
- ⚠ simulator fidelity gaps listed; device-side cases not yet implemented
- ⚠ **RM2 resource budgets not measured** — impossible without hardware or an ADR-002 decision

### Characterize and enforce ☑

- ☑ 13 characterization tests pinning the transition table, denial precedence, classification counts, flag defaults, storage arithmetic, capability permissions, schema surface, and intent/grant lifetimes
- ☑ 6 architecture tests enforcing dependency direction and forbidden imports in CI

### Refactor incrementally ☐

Slices, each independently reversible and green:

- ☑ **1. `git init`, baseline commit, tag `phase-0.5-audit`** — done 2026-08-12. A true `phase-0-baseline` tag is not achievable retroactively (the repository was created after the audit); the Phase 0 evidence lives in [PHASE_0_REPORT.md](docs/development/PHASE_0_REPORT.md) instead. Every slice from here gets its own commit.
- ☑ 2. architecture + characterization tests
- ☑ 3. `StorageProfile` parameter instead of hard-coded WAL ([ADR-005](docs/adr/ADR-005-device-storage-profile.md)) — device profile is conservative until U12 is measured
- ☑ 4. split `DeviceProvider` into `DeviceIntrospection` (on-device) and `RemoteDeviceTransport` (companion)
- ☑ 5. `CredentialStore` port + `Redacted` moved into the core ([ADR-004](docs/adr/ADR-004-device-credentials.md)); implementations deferred to the setup flow that needs them
- ☑ 6. `Clock` port; repositories and migrations use it
- ☑ 7. cross-compilation CI job for the portable crates
- ◐ 8. cross-C toolchain so `marginalia-database` builds for ARM (U17) — **needs Docker or `cross`, neither available on the machine where Phase 0.5 ran.** Not wired into CI rather than shipping an unverified step.
- ☑ 9. simulator device-side faults: power loss, corruption, truncation, storage pressure, clock skew
- ⚠ 10. minimal `apps/remarkable` smoke app — **gated on ADR-002**

Slices 1–9 were worth doing under every outcome of ADR-002, and all but slice 8
are complete. Slice 10 is gated.

### Deliberately not done

Not renaming `packages/` → `crates/`; not creating a speculative `application/`
layer with no second consumer; not creating `apps/remarkable`; not touching a
real device; not modifying any Phase 0 production code.

**Exit:** Phase 0 behaviour green; boundaries enforced by CI; minimal RM2 app
cross-compiles and passes simulator packaging/install/uninstall/rollback tests;
audit report approved; **ADR-002 decided**.

## Phase 1 — RM2 native shell, local storage, and Safe Mode ⚠ BLOCKED

Blocked on [ADR-002](docs/adr/ADR-002-remarkable-ui-and-runtime.md) (U11).

- ⚠ E-Ink UI with explicit refresh policy — shape depends entirely on ADR-002
- ☐ local navigation, empty/loading/error/offline states, persistent Safe Mode indicator
- ☐ Marginalia-owned SQLite with a measured journal mode (U12)
- ☐ crash-safe migrations, activity journal, storage reserve, corruption recovery
- ☐ RM2 credential adapter, least-privilege, redaction-tested
- ☐ device/app/firmware identity without probing private internals
- ☐ simulator: low storage, forced reboot, process kill, corrupt DB, clock skew, no Wi-Fi
- ☐ install/update/rollback/uninstall on simulator, then on a real RM2 (U13)

**Exit:** the app runs locally with Wi-Fi and desktop absent; owns and recovers
its data; uninstalls cleanly; writes no native/system path.

## Phase 2 — Direct Zotero metadata synchronization on RM2 ◐

Progress so far, all portable and hardware-free:

- ☑ credentials model — library ID, library kind, key, kept distinct
- ☑ setup flow that verifies before storing, and never stores a rejected key
- ☑ key-only setup: the library is discovered via `/keys/current`, not asked for
- ☑ `HttpZoteroClient` behind the `http` feature (off by default; U16)
- ☑ `SyncCursor` / `SyncPlanner`: incremental versions, pagination, deletions
- ☑ watermark advances only after the last page commits, so an interrupted
      sync re-runs a page rather than skipping it
- ☑ `BackoffPolicy`: `Retry-After` wins, exponential with a ceiling otherwise,
      and a permanent failure is never retried
- ☑ `SyncTally` reporting `pdfs_transferred`, always zero for a metadata sync
- ☐ wiring the planner to the database and the journal
- ☐ collections and tags
- ☐ the agent exposing `sync` and the Zotero setup command
- ☐ TLS on the device (U16)

The v1 desktop-side plan below is superseded in target but not in substance:
the sync engine, firewall and journal are unchanged; the runtime moves to the
device, and the local-Zotero-SQLite adapter becomes desktop-only.
Additional work: on-device guided API-key setup, TLS on device (U16),
bounded concurrency, and zero-PDF-byte network assertions.

### Superseded v1 checklist


- ☐ `ZoteroAdapter` — local SQLite (read-only) + Web API v3
- ☐ Credentials in OS secure storage
- ☐ `SyncPlanner` / `SyncExecutor` with the `MetadataOperation` firewall
- ☐ Incremental sync, pagination, backoff, retries, offline
- ☐ Collections, tags, attachment **availability** resolution
- ☐ Library UI: list, filters, states, Zotero metadata panel
- ☐ Sync journal + Activity view
- ☐ Safety tests S8, S9 — metadata sync transfers zero PDFs

**Exit:** the whole library is browsable with zero bytes moved to any device;
automatic PDF transfer is not expressible in the type system, and tests prove it.

## Phase 3 — Explicit, on-demand PDF download to RM2 ☐

Retargeted: the device downloads from Zotero directly rather than receiving a
file from the desktop. The gate structure — one attachment, explicit intent,
preflight, checksum, snapshot, atomic publish, rollback — is unchanged.

### Superseded v1 checklist


Gated on U1. The first phase that writes anything.

- ☐ ADR-003 transport decision
- ☐ PDF validation + checksums
- ☐ `SafetySnapshot` create/verify
- ☐ Transfer pipeline with post-verification and rollback
- ☐ `ExplicitUserIntent` + Send/Remove commands, confirmation UX
- ☐ `DocumentMapping` registration, idempotency
- ☐ Storage guard + reserve
- ☐ Safety tests S2–S5, S7, S10, S11, S14
- ☐ Full simulator suite green **before** any real-device test
- ☐ Real-device validation with a throwaway PDF

**Exit:** one document transfers, verifies, and rolls back correctly under every
simulated fault.

## Phase 4 — Safe local document discovery and compatibility matrix ☐

- ☐ Device detection, identity, firmware parsing
- ☐ Capability layer + `matrix.toml` loader (empty `tested_at` ⇒ `UNKNOWN`)
- ☐ Storage read + `DeviceStorageManager` projections
- ☐ Document listing, mapping reconciliation, foreign-document protection
- ☐ Device Dashboard UI, Safe Mode indicator
- ☐ Read-only probing session on the real device → **resolve U1, U2, U6**
- ☐ Safety tests S1, S15

**Exit:** we can describe the device accurately and cannot write to it at all.

## Phase 5 — Local highlights extraction ☐

⚠ Gated additionally on **U15**: ADR-001 chose PDFium for a desktop target and
there is no obvious prebuilt armv7 binary.


Gated on U3, U7, U8, U10.

- ☐ Annotation file ingest (read-only)
- ☐ `.rm` parsing behind a versioned parser with honest failure
- ☐ Coordinate mapping, round-trip property tests
- ☐ PDF text extraction with geometry (`pdfium-render`)
- ☐ Highlight↔text intersection, reading order, context capture
- ☐ Highlight model, storage, Highlight view UI
- ☐ Derived annotated PDF export — flag `native_pdf_annotations`, default OFF
- ☐ Safety test S6; original-immutability property test on every fixture

**Exit:** highlights become text with provenance; originals are provably
byte-identical.

## Phase 6 — Autonomous Annotation Inbox ☐

- ☐ Unified annotation projection + inbox UI
- ☐ Filters (kind, type, date, document, author, collection, tag, year)
- ☐ Actions: open source, copy, edit, tag, send to Zotero, archive
- ☐ Source navigation (document → page → position)
- ☐ Zotero export path

## Phase 7 — Resource-bounded local search ☐

- ☐ FTS5 index over PDF text, highlights, notes, Zotero metadata
- ☐ `SearchQueryParser`, `SearchRanker`, facets
- ☐ Provenance on every result — no result without a source
- ☐ Incremental reindexing

## Phase 8 — Local side notes and sticky notes ☐

- ☐ Side Notes (desktop): anchors, Markdown, highlight links
- ☐ Sticky Notes (desktop): positioning, overlay rendering
- ☐ Zotero note/annotation export
- ☐ No device UI integration, no original-PDF modification

## Phase 9 — Tags and Zotero bridge ☐

- ☐ Zotero ↔ Marginalia tags
- ☐ Mapping model, normalisation, conflict UI, confirmation requirement
- ☐ reMarkable tag **read**
- ☐ reMarkable tag **write** only if U5 resolves `SUPPORTED`

## Phase 10 — RM2 command palette and quick switcher ☐

- ☐ `CommandRegistry`, `Command`, `CommandContext`, `CommandExecutor`
- ☐ ⌘K / Ctrl+K, fuzzy search, recents, keyboard-first
- ☐ Features register their own commands — no god component

## Phase 11 — Direct annotations/notes export to Zotero ☐

☐ stable export records, preview, explicit Send, local outbox, retry,
idempotency, conflict detection, cursors/receipts, safe credential expiry.

## Phase 12 — Optional desktop companion / power mode ☐

Desktop becomes an optional client of the same application contracts, not the
backend. It must be possible to uninstall it indefinitely without disabling any
essential RM2 workflow.

## Phase 13 — Production hardening and release ☐

☐ firmware matrix validated with evidence; upgrade/rollback across released
versions; manifest/signature/partial-install recovery; threat modelling; parser
fuzzing; long-duration battery/memory/power-loss tests; recovery instructions
that never require touching kernel/boot/system.

## Retired from v1 — optional RM companion

The v1 plan ended with an optional companion *running on the device*. The v2
product decision makes the device the primary runtime, so this phase no longer
exists as written. What survives of it is the constraint set: anything that
ever runs on the device must be isolated, removable, must not replace
`xochitl`, must not patch system files, and must not affect boot or updates.

## Non-goals

Home-screen replacement · PDF reader replacement · notebook replacement ·
custom firmware/kernel · aggressive RM daemons · RM app store · cloud
collaboration · AI summaries · OCR competing with native handwriting search ·
automatic bulk PDF sync · automatic deletion to save space.

## Future-safe, not now

Other reMarkable models · other citation managers · Obsidian/Markdown export ·
native Zotero annotation creation · richer companion · semantic search ·
optional local AI. The adapter boundaries make these possible; none is built in
V1.
