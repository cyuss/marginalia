# Marginalia — Roadmap

Status: **the core loop reads real highlights off real hardware**
Target: **reMarkable 2**, as the primary and only required runtime
Repository: one Cargo workspace, no JavaScript

## What this is

Marginalia turns what you already do on a reMarkable — reading and highlighting
— into durable, portable notes, **without changing the device or how you use
it**.

That sentence is the whole product, and it is narrower than the one this
document used to describe. Two findings narrowed it, both from a device rather
than from reasoning:

1. **There is no way to draw on the screen** without modifying software that
   belongs to reMarkable. So there is no Marginalia interface on the device.
   The native reader is the interface, and it is untouched.
   ([ADR-002](docs/adr/ADR-002-remarkable-ui-and-runtime.md), which now also
   covers the binary-patching route and why it is refused rather than
   impossible.)
2. **Highlighted text is stored as text**, in
   `<uuid>.highlights/<page>.json` — not buried in stroke geometry, as the desk
   research had feared. Reading it is reading a JSON file.
   ([HARDWARE_VALIDATION.md](docs/remarkable/HARDWARE_VALIDATION.md))

The first removed a class of features permanently. The second removed the
project's largest risk. What is left is one loop, and everything below serves
it:

```
read and highlight        extract              review              keep
in the native reader  →   what the device  →   as documents   →   as Markdown,
(unchanged)               already stored       and digests         or in Zotero
```

**Where documents come from is a plug-in concern.** A `LibraryProvider` supplies
source-neutral items; a folder needs no network, and Zotero is the richest
source implemented. Nothing above the port knows which is in use. Zotero is not
what this tool is for; it is one place documents can come from.

## Kept and excluded

The full reasoning is in the README. In short:

**Kept** — the on-device agent; highlight extraction; persistence with history;
Markdown and JSON export; generated review documents; explicit document
transfer; library sources as ports; the safety model, capability matrix and
one-command removal; the request form.

**Excluded permanently** — any Marginalia interface on the device (split view,
sidebar, overlays, a command palette on a device with no keyboard); patching
`xochitl`; writing tags into the device's own metadata; annotating original PDFs
on the device; OCR; a package manager or system service; cloud sync of
Marginalia's data; and any automatic file transfer.

**Removed 2026-08-13** — the Tauri desktop application. Six screens of mock
interface wired to nothing, which had never once built, costing two CI jobs. The
terminal interface (`apps/tui`) replaces it, and it does what that app never
did: install, check, configure and remove, by driving the tools that already
exist.

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
- ☑ 8. cross-C toolchain (U17) — `tools/device/build-in-docker.sh` produces a running ARM binary; verified under emulation
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

## Phase 1 — RM2 agent, local storage, and Safe Mode ◐

Unblocked by ADR-002. The shape is a headless agent, not a native shell.

- ☑ the agent binary, with a startup guard refusing any home outside its own
- ☑ device-profile storage: rollback journal, `synchronous = FULL`
- ☑ `status` / `init` / `doctor`, reporting what it may and may never do
- ☑ manifest-based install, and a reset that verifies the device is back to stock
- ☐ ~~E-Ink UI with explicit refresh policy~~ **superseded**: no UI, by decision
- ☐ local navigation, empty/loading/error/offline states, persistent Safe Mode indicator
- ☐ Marginalia-owned SQLite with a measured journal mode (U12)
- ☐ crash-safe migrations, activity journal, storage reserve, corruption recovery
- ☐ RM2 credential adapter, least-privilege, redaction-tested
- ☐ device/app/firmware identity without probing private internals
- ☐ simulator: low storage, forced reboot, process kill, corrupt DB, clock skew, no Wi-Fi
- ☐ install/update/rollback/uninstall on simulator, then on a real RM2 (U13)

**Exit:** the app runs locally with Wi-Fi and desktop absent; owns and recovers
its data; uninstalls cleanly; writes no native/system path.

## Phase 2 — Library sources on the device ◐ (unblocked)

- ☑ `LibraryProvider` port and a source-neutral `LibraryItem` in `core`
- ☑ **folder** provider — no network, no account, works on the device
- ☑ cross-source identity by strong identifier only, never by title
- ☐ a `source add` command in the agent, and folder items in the database

### Zotero, the first rich source

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
- ☑ `MetadataApplier`: a page applies entirely or not at all, and re-applying
      a page is safe
- ☑ `SyncStateRepository`: the watermark is a separate write, made last, so a
      crash between the final page and the final commit costs a re-fetch and
      never data
- ☑ migration 0002 `zotero_sync_state`, with a forward-migration test
- ☑ the journal: jobs, operations, and `already_done` so a replayed request
      is a no-op rather than a second download
- ☑ `SyncRunner` (`packages/sync`): the page loop, with the cursor moved last
      and only once
- ☑ `fetch_items` over HTTP: server-driven pagination from the `Link` header,
      tolerant item parsing so a Zotero schema addition is a display gap and
      not an outage
- ☑ the agent: `zotero connect` / `use` / `disconnect` and `sync`, behind a
      `network` feature (off by default while U16 is open)
- ☐ collections and tags
- ☑ tags, read off the item payload so they cost no extra request, and
      deduplicated per page
- ☑ collections, with hierarchy kept as a parent key rather than resolved —
      a parent may arrive on a later page, and guessing would reparent a
      user's collection
- ☑ TLS on the device (U16) — works on armv7 with roots bundled in the binary,
      verified against the real Zotero API from an ARM environment with no
      system certificate store
- ☐ a real device: install and run there

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

## Phase 4 — Safe local document discovery and compatibility matrix ◐

- ☑ `discovery`: a read-only `DeviceReport` — model, raw firmware string,
      storage, and how many documents are ours versus the user's
- ☑ ownership classification as a pure function of two sets, defaulting to
      **theirs** so a classification bug errs towards leaving things alone
- ☑ divergence reporting: a document the user deleted is reported, never
      restored
- ☑ largest-documents view for the storage screen — reporting only, since
      Marginalia never deletes to make room
- ☑ unreadable storage is treated exactly like a full device
- ☐ a real transport to read any of it from (gated on a device)

### Superseded v1 checklist

- ☐ Device detection, identity, firmware parsing
- ☐ Capability layer + `matrix.toml` loader (empty `tested_at` ⇒ `UNKNOWN`)
- ☐ Storage read + `DeviceStorageManager` projections
- ☐ Document listing, mapping reconciliation, foreign-document protection
- ☐ Device Dashboard UI, Safe Mode indicator
- ☐ Read-only probing session on the real device → **resolve U1, U2, U6**
- ☐ Safety tests S1, S15

**Exit:** we can describe the device accurately and cannot write to it at all.

## Phase 5 — Highlights ◐ (reading done)

The phase this project was most afraid of, and the one hardware made small.

- ☑ Highlight ingest, read-only, from `<uuid>.highlights/<page>.json`
- ☑ Page numbering for `.content` formatVersion 1 **and** 2, with an unverified
  version refused rather than guessed
- ☑ `marginalia highlights` — list, show, export Markdown, emit JSON
- ☑ Verified on hardware: 26 documents, 2,624 highlights, nothing written
- ☐ Persist highlights with `extraction_version`, so a format correction can be
  re-run instead of silently disagreeing with older rows
- ☐ History: what changed since the last extraction
- ☐ Handwritten strokes via a versioned `.rm` v6 parser — text from strokes is
  a separate question and may never be answered

**Dropped from this phase.** Highlight↔text intersection, coordinate mapping and
PDF text extraction with geometry: all were needed only if the text had to be
recovered from stroke geometry. It does not. PDFium on armv7 (U15) is no longer
on the critical path.

**Exit:** highlights are stored with provenance and survive a format change;
originals are provably byte-identical.

## Phase 6 — Review documents ☐

The first thing Marginalia puts on the screen — by writing a document the native
reader opens, which is the only screen it will ever have.

- ☐ A digest per document: your highlights, in reading order, with pages
- ☐ A library index listing what has been highlighted and when
- ☐ Generated into your library only when asked, never silently
- ☐ Regeneration is idempotent: no duplicate documents accumulating
- ☐ Removal takes the generated documents with it

## Phase 7 — The request form ☐

[ADR-006](docs/adr/ADR-006-on-device-interaction.md). The only way to ask for
something on a device with no interface: tick a box with the stylus.

- ☐ Generate an index with a tick box per entry
- ☐ Read ticks back from the annotation layer, generation-scoped and idempotent
- ☐ An ambiguous mark is never guessed — it is reported and ignored
- ☐ `FormRequest` → `ExplicitUserIntent`, which is what a transfer requires

## Phase 8 — see Phase 3

Explicit document transfer is Phase 3 and always was; a second entry here was a
duplicate produced while reorganising. Phase 7's request form is what supplies
the `ExplicitUserIntent` that Phase 3 requires, so the two are done in that
order.

## Phase 9 — Search, within the device's means ☐

- ☐ FTS5 over highlights and library metadata
- ☐ Results delivered as a generated document, like everything else
- ☐ Provenance on every result — no result without a source
- ☐ Incremental reindexing, bounded memory

## Phase 10 — Tags, read-first ☐

- ☐ Marginalia tags, and a mapping model with normalisation and conflicts
- ☐ reMarkable tag **read**
- ☐ reMarkable tag **write** only if U5 resolves SUPPORTED — a read-only bridge
  is an acceptable final answer

## Phase 11 — Export to Zotero ☐

☐ stable export records, preview, explicit send, local outbox, retry,
idempotency, conflict detection, receipts, safe credential expiry.

## Phase 12 — Production hardening and release ☐

☐ firmware matrix validated with evidence; upgrade and rollback across released
versions; manifest and partial-install recovery; threat modelling; parser
fuzzing; long-duration battery, memory and power-loss tests; recovery
instructions that never require touching kernel, boot or system.

## Removed from the plan

Phases that existed because the project once expected a screen it will not get,
or a desktop application it no longer has.

| Was | Why it is gone |
|---|---|
| Annotation Inbox with filters and actions | It was an inbox **UI**. What survives is Phase 6: the same information, as a document. |
| Side notes and sticky notes with overlay rendering | Overlays need the display. |
| Command palette and quick switcher (⌘K, "keyboard-first") | The reMarkable has no keyboard and no interface Marginalia may reach. This one had been sitting in the plan long after ADR-002 made it impossible. |
| Optional desktop companion / power mode | The Tauri app is deleted; `apps/tui` is the companion, and it runs in a terminal. |
| Derived annotated PDF export on the device | A PDF stack on armv7 to reproduce text that is already text. Markdown export does the job. |

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
