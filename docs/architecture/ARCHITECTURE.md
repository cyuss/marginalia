# Marginalia — Architecture

Status: **Draft v1 — awaiting validation**
Last updated: 2026-08-12

---

## 1. What Marginalia is

Marginalia is a **local-first desktop application** that connects a Zotero
research library to a stock reMarkable 2, and brings the resulting highlights
and handwritten annotations back into a searchable, Zotero-linked knowledge
layer.

It is **not** a reMarkable replacement, a reMarkable modification, or a Zotero
replacement. It is the missing layer between them:

```
Library → Reading → Annotation → Knowledge → Zotero
```

## 2. The three invariants

Every design decision in this document is downstream of three invariants. If a
change would violate one of them, the change is wrong — not the invariant.

### INV-1 — The reMarkable stays stock

Removing Marginalia, crashing it, or never launching it again must leave the
device exactly as usable as a device that never met Marginalia. No system
partition writes, no `xochitl` patching, no bootloader/kernel changes, no
firmware-update interference.

### INV-2 — Metadata sync never moves a PDF

A Zotero sync transfers bibliographic knowledge. Only an explicit, per-document
`Send to reMarkable` user action transfers a file. These are separate code
paths, separate job types, and separate UI verbs — enforced structurally, not by
convention. See [§7](#7-the-transfer-firewall).

### INV-3 — Originals are immutable

The Zotero-owned PDF on disk is opened read-only, always. Annotation rendering
produces a *derived* file. A failed derivation is discarded; it can never
partially overwrite a source.

## 3. Deployment shape

```
                         DESKTOP  (macOS / Windows / Linux)

                              Zotero  (local SQLite / Web API)
                                 │
                                 ▼
             ┌───────────────────────────────────────────┐
             │            Marginalia (Tauri 2)           │
             │                                           │
             │   React + TS UI  ◄── IPC ──►  Rust core   │
             └───────────────────────┬───────────────────┘
                                     │
        ┌───────────────┬────────────┼────────────┬───────────────┐
        ▼               ▼            ▼            ▼               ▼
   Zotero Engine  Annotation   PDF Engine     Search        SafetyManager
                   Engine                    (FTS5)        (gates everything
        │               │            │            │          below)
        └───────────────┴────────────┼────────────┘               │
                                     ▼                            │
                                  SQLite                          │
                                     │                            │
                                     ▼                            ▼
                              Device Bridge ◄──── every call is authorised
                                     │             by SafetyManager first
                              safe operations only
                                     ▼
                              reMarkable 2 — STOCK
```

Target split: **~90% desktop / ~10% optional companion**. V1 target: **100%
desktop**. Nothing runs on the reMarkable in V1.

## 4. Monorepo layout

```
marginalia/
├── apps/
│   ├── desktop/                  Tauri 2 shell — React UI + Rust binary
│   └── remarkable-companion/     Phase 10 only. Empty placeholder in V1.
│
├── packages/
│   ├── shared-types/             TS types generated from Rust (ts-rs)
│   ├── core/                     domain entities, state machines, errors
│   ├── database/                 SQLite access, migrations, repositories
│   ├── safety/                   SafetyManager, classification, snapshots
│   ├── zotero/                   ZoteroAdapter + sync engine
│   ├── remarkable/               DeviceProvider, capabilities, compat matrix
│   ├── annotations/              .rm parsing, highlight geometry, extraction
│   ├── pdf/                      text extraction, geometry, derived PDFs
│   ├── search/                   FTS5 index, query parser, ranker
│   ├── sync/                     job orchestration, journal, conflicts
│   └── ui/                       design system, primitives, command palette
│
├── features/                     vertical slices, UI + wiring per feature
│   ├── zotero-sync/  highlight-extractor/  side-notes/  sticky-notes/
│   ├── annotation-inbox/  annotation-search/  zotero-metadata/
│   └── zotero-tags/  command-palette/
│
├── tests/
│   ├── unit/ integration/ e2e/ safety/ fixtures/ remarkable-simulator/
│
├── docs/  scripts/  .github/
└── CLAUDE.md README.md ROADMAP.md SECURITY.md CONTRIBUTING.md LICENSE
```

**Rust crates** live under `packages/*` as a Cargo workspace; **TS packages**
under the same paths as a pnpm workspace. A package is either a Rust crate, a TS
package, or both (`core` is both: Rust source of truth + generated TS types).

Dependency rule — strictly acyclic, pointing inward:

```
features/*  →  packages/ui, packages/shared-types
apps/desktop → features/*, packages/*
packages/{zotero,remarkable,annotations,pdf,search,sync} → core, database, safety
packages/{core, safety}  →  (nothing above them)
```

`packages/core` depends on nothing else in the repo. `packages/safety` depends
only on `core`. Nothing may reach the device except through
`packages/remarkable`, and `packages/remarkable` cannot execute a write without
a `SafetyManager` authorisation token. This is enforced by the type system —
see [§6](#6-the-safety-boundary).

## 5. Layered model

| Layer | Contents | Knows about the device? |
|---|---|---|
| **Domain** | `core` — entities, states, transitions, pure functions | No |
| **Application** | `sync`, `annotations`, `search` — use cases, orchestration | Only via ports |
| **Ports** | `BibliographyProvider`, `DeviceProvider`, `AnnotationProvider`, `StorageProvider`, `SearchProvider` | Interface only |
| **Adapters** | `zotero`, `remarkable`, `pdf`, `database` | Yes, concretely |
| **Presentation** | `apps/desktop`, `features/*`, `ui` | No — commands only |

The domain layer is pure and exhaustively testable without a device, a network,
or a filesystem. That is what makes the simulator strategy work.

## 6. The safety boundary

`SafetyManager` is not advisory. It is a **capability issuer**, and the device
API is shaped so that it cannot be bypassed:

```rust
// packages/remarkable — sketch
pub struct WriteGrant {           // constructible ONLY inside packages/safety
    op: DeviceOperation,
    device: DeviceId,
    issued_at: Timestamp,
    snapshot: Option<SafetySnapshotId>,
    _private: PhantomData<NotConstructibleElsewhere>,
}

impl DeviceProvider {
    // GREEN — no grant needed
    async fn read_device_info(&self) -> Result<DeviceInfo>;
    async fn list_documents(&self) -> Result<Vec<RemoteDocument>>;
    async fn read_storage(&self) -> Result<StorageInfo>;

    // YELLOW — grant is a required parameter, not a check inside the body
    async fn upload_document(&self, grant: &WriteGrant, doc: ValidatedPdf) -> Result<...>;
}
```

Because `WriteGrant` has a private field and a private constructor, **no code
outside `packages/safety` can fabricate one**. A developer cannot accidentally
write to the device by forgetting a check; there is no code path that compiles
without a grant. Grants are single-use, operation-scoped, device-scoped, and
time-limited.

`SafetyManager.authorize(op)` runs, in order, and **fails closed** at any step:

1. Feature flag enabled for this operation class?
2. Device identified and matches the grant target?
3. Firmware known, and matrix status for this feature is `SUPPORTED`?
   (`UNKNOWN` / `READ_ONLY` / `UNSUPPORTED` → deny)
4. Safe Mode constraints satisfied?
5. Preconditions: storage headroom incl. reserve, PDF validity, checksums?
6. Snapshot created and verified, if the operation class requires one?
7. Rollback plan exists and is executable?

Full detail: [`docs/safety/SAFETY_MODEL.md`](../safety/SAFETY_MODEL.md) and
[`docs/safety/DEVICE_WRITE_POLICY.md`](../safety/DEVICE_WRITE_POLICY.md).

## 7. The transfer firewall

INV-2 is enforced by **type separation**, not by discipline:

- Sync produces a `SyncPlan` whose operations are of type `MetadataOperation` —
  an enum that has **no variant capable of expressing a file transfer**.
- `Send to reMarkable` produces a `TransferPlan` containing `TransferOperation`.
- `SyncExecutor` accepts only `MetadataOperation`. `TransferExecutor` accepts
  only `TransferOperation`, and requires an `ExplicitUserIntent` value that is
  minted solely by the UI command handler bound to the Send button.

There is therefore no expressible program in which "sync" enqueues a transfer.
The corresponding safety test (`metadata sync → zero PDF transfers`) is a
regression guard on top of a structural guarantee, not the only line of defence.

## 8. Data flow — the canonical loop

```
1. Zotero → Marginalia      metadata sync        (network/DB read, no files)
2. User   → Send            explicit transfer    (one PDF, gated, verified)
3. RM2    → reading         native, untouched
4. RM2    → Marginalia      annotation ingest    (read-only device access)
5. Marginalia               extraction + index   (derived artefacts only)
6. Marginalia → Zotero      structured export    (explicit, per user action)
```

Steps 1, 4, 5 are automatic-capable. Steps 2 and 6 are **always** explicit user
actions.

## 9. Persistence

SQLite, WAL mode, versioned forward-only migrations, foreign keys ON. FTS5 for
search. Identity is never a filename — every document is keyed by
`DocumentMapping` linking Zotero keys ↔ local id ↔ device id ↔ checksums.
Schema: [`SQLITE_SCHEMA.md`](./SQLITE_SCHEMA.md).
Entities: [`DOMAIN_MODEL.md`](./DOMAIN_MODEL.md).

## 10. State machines

- Document lifecycle: [`DOCUMENT_STATE_MACHINE.md`](./DOCUMENT_STATE_MACHINE.md)
- Sync/job lifecycle: [`SYNC_STATE_MACHINE.md`](./SYNC_STATE_MACHINE.md)
- Device capabilities: [`DEVICE_CAPABILITY_MODEL.md`](./DEVICE_CAPABILITY_MODEL.md)

State transitions are implemented as exhaustive Rust `match` over enums; illegal
transitions are unrepresentable or return a typed error. No boolean soup
(`isDownloaded`, `hasPdf`, `isSynced`) anywhere in the domain.

## 11. Technology

See [`ADR-001-backend-stack.md`](./ADR-001-backend-stack.md) for the reasoning.
Summary: Tauri 2 + React 19 + TypeScript + Vite + Tailwind + Radix + TanStack
Query, Rust core, `pdfium-render` for PDF text geometry, `lopdf` for derived
annotated PDFs, no Python sidecar in V1 (interface preserved so one can be added
without redesign).

## 12. Security and privacy posture

No account, no Marginalia server, no telemetry, no analytics, no document or
annotation upload. Zotero API keys live in OS secure storage (Keychain /
Credential Manager / Secret Service), never in SQLite, never in logs, never in
config files. See [`SECURITY.md`](../../SECURITY.md).

## 13. Error and observability model

Errors are typed domain values, not strings. Every user-facing error must answer:
what happened, what was affected, is data safe, what can I do. Structured logs at
`DEBUG/INFO/WARN/ERROR/SAFETY`; the `SAFETY` channel is separately persisted and
auditable. Secrets and note contents are never logged.

## 14. What we deliberately do not build

Home-screen replacement, PDF reader replacement, notebook replacement, custom
firmware/kernel, RM background daemons, cloud collaboration, AI summaries, OCR
competing with native handwriting search, automatic bulk sync, automatic
deletion. See [`ROADMAP.md`](../../ROADMAP.md) §Non-goals.
