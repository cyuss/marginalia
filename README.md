<div align="center">

# Marginalia

**A local-first research, reading and annotation companion for reMarkable.**

Connect your Zotero library to your reMarkable 2 — and get your highlights and
handwritten notes back as searchable, citable knowledge.

[Install](docs/INSTALL.md) · [Architecture](docs/architecture/ARCHITECTURE.md) ·
[Safety model](docs/safety/SAFETY_MODEL.md) · [Roadmap](ROADMAP.md) ·
[Contributing](CONTRIBUTING.md)

</div>

---

> ### Status: Phase 0 — foundation
>
> The domain model, safety layer, database, device simulator and application
> shell are implemented and tested. **Zotero and reMarkable connections are not
> built yet** (Phases 1 and 2). This build contains no device transport code
> whatsoever and cannot modify a reMarkable.
>
> Marginalia is being built in public, phase by phase. See [ROADMAP.md](ROADMAP.md).

---

## The problem

If you read research on a reMarkable, you know the gap.

The device is excellent at reading and writing. It is not a research tool. Your
papers arrive as `attention.pdf` with no author, no year, no journal, no
collection. You highlight a passage that turns out to matter, and three months
later you cannot remember which of forty PDFs it was in. Your handwritten notes
live on the device; your bibliography lives in Zotero; nothing connects them.

The usual answers are worse than the problem. Tools that "fix" the reMarkable
by patching its system software risk the device itself. Tools that sync
everything automatically fill 8 GB of storage with papers you never intended to
read there.

## What Marginalia does

It adds the missing layer, and only that layer:

```
Zotero  ──── metadata ────►  Marginalia  ──── you press Send ────►  reMarkable 2
                                  ▲                                      │
                                  └──── highlights, handwritten notes ────┘
                                  │
              Annotation Inbox · Search · Metadata · Export to Zotero
```

**Library → Reading → Annotation → Knowledge → Zotero.**

Your reMarkable stays a reMarkable. Handwriting, pen tools, notebooks, PDF
reading, page navigation, native tags, native search — all of it stays native
and untouched. Marginalia does not recreate any of it.

## Three promises

These are not aspirations. They are enforced by the architecture, and each is
covered by tests that must pass for any change to merge.

### 1. Your reMarkable stays completely stock

Marginalia never patches `xochitl`, never touches the bootloader, kernel or
system partitions, never replaces system libraries, and never interferes with
firmware updates. It installs nothing on the device.

Uninstall Marginalia and your reMarkable is exactly as it was — because nothing
was ever changed.

### 2. Syncing moves knowledge, not files

This is the promise most tools get wrong, so it is worth being precise:

> **Sync** updates metadata, tags, collections and annotations.
> **Send to reMarkable** transfers one specific PDF, because you asked.

A sync will never copy a PDF to your device. Not for one paper, not for five
hundred. You can browse your entire Zotero library inside Marginalia — every
title, author, collection and tag, and whether a PDF is available — with zero
bytes on your reMarkable.

Every sync report shows the transfer count, including when it is zero:

```
Sync completed

Updated metadata        12
New Zotero items         4
Updated tags             7
Annotations imported     8
PDFs transferred         0     ← always shown
```

### 3. Your original PDFs are never modified

Source files are opened read-only, always. Annotated versions are separate
derived files. A failed derivation is discarded; it cannot partially overwrite
anything.

---

## How the promises are enforced

Most software makes safety promises in documentation and enforces them with
code review. Marginalia enforces them with the type system, so that breaking one
is a compile error rather than a bug report from someone whose device stopped
working.

### Writes require a token that almost nothing can create

Every function that changes something on a device takes a `WriteGrant`:

```rust
// reads — no grant needed
fn list_documents(&self) -> DeviceResult<Vec<RemoteDocument>>;

// writes — the grant is a required parameter, not a check inside the body
fn upload_document(&mut self, grant: &WriteGrant, pdf: &ValidatedPdf, name: &str)
    -> DeviceResult<RemarkableDocumentId>;
```

`WriteGrant` holds a field of a private type. Rust therefore forbids
constructing one anywhere outside the safety crate — there is no struct
literal, no `Default`, no deserialisation path. The only way to obtain one is
`SafetyManager::authorize()`, which runs every check and **fails closed** at
the first doubt.

A contributor who adds a new device write and forgets the safety check does not
introduce a dangerous bug. Their code does not compile.

### "Sync" and "transfer" are different types

```rust
enum MetadataOperation {         // what a sync may do
    UpsertZoteroItem { .. },
    UpsertAttachmentAvailability { .. },   // records a fact, moves nothing
    // ← there is deliberately NO variant that can move a file
}

enum TransferOperation {         // what pressing Send may do
    UploadPdf { intent: ExplicitUserIntent, .. },
    RemoveDeviceDocument { intent: ExplicitUserIntent, .. },
}
```

The sync executor accepts only the first type. There is no program in which a
metadata sync transfers a PDF — not because we remembered to check, but because
the sentence cannot be written. `ExplicitUserIntent` is created only by the
command handler behind a button you pressed, is scoped to one document and one
action, expires, and is consumed on use.

Two further independent guards back this up: the document state machine (only
one edge reaches a transfer, and only via an explicit user event) and a SQL
`CHECK` constraint (a scheduled transfer job is rejected by the database).

### Untested firmware means read-only

reMarkable firmware evolves. No feature code in Marginalia parses a firmware
string; it asks a capability layer, which answers from a versioned matrix.

Anything not verified on real hardware resolves to `UNKNOWN`, and `UNKNOWN`
never permits a write. A matrix entry claiming `SUPPORTED` without a test date
is loaded as `UNKNOWN` regardless — optimism in a data file cannot grant
permissions. A user override can *restrict* a capability but never expand one:
there is no "enable writes anyway" switch.

If your device updates its firmware overnight, Marginalia drops to read-only
and explains why. That is the correct behaviour.

### Marginalia only touches what it put there

A device document whose UUID is not in Marginalia's own mapping table belongs
to you, and is read-only forever. Your notebooks are never written to under any
circumstance. Nothing is ever deleted automatically — not to save space, not to
resolve a conflict, not ever.

### The whole device write policy, in full

Marginalia may write to a reMarkable only to:

1. add one PDF you explicitly sent;
2. remove one document Marginalia itself transferred, on confirmation;
3. set native tags on a document Marginalia manages, from a mapping you confirmed;
4. replace a document Marginalia transferred with its annotated version
   (feature-flagged off by default).

Four operations. All user-initiated, one document at a time, all reversible,
all verified afterwards by checksum, all with a tested rollback. Anything else
is forbidden — see [DEVICE_WRITE_POLICY.md](docs/safety/DEVICE_WRITE_POLICY.md).

---

## Features

Built and tested today (Phase 0):

- The domain model and both state machines
- The safety layer: `SafetyManager`, write grants, classification, snapshots, feature flags
- SQLite storage with versioned migrations and schema-level safety constraints
- The firmware capability layer and compatibility matrix
- A deterministic reMarkable simulator with fault injection
- The mandatory safety suite
- The application shell

Planned, phase by phase:

| Phase | Feature |
|---|---|
| 1 | **Zotero sync** — metadata, collections, tags, attachment availability |
| 2 | **Device connection** — detection, firmware, capabilities, storage (read-only) |
| 3 | **Send to reMarkable** — validated, verified, reversible transfer |
| 4 | **Highlight extractor** — turn highlights into quotable text with page and position |
| 5 | **Annotation Inbox** — every note from every document, in one place |
| 6 | **Search** — across PDF text, highlights, notes and Zotero metadata |
| 7 | **Side notes & sticky notes** — structured, anchored, desktop-side |
| 8 | **Tag bridge** — Zotero ↔ reMarkable tags, never silently merged |
| 9 | **Command palette** — ⌘K / Ctrl+K |
| 10 | Optional companion — only if something genuinely cannot live on the desktop |

---

## What it deliberately does not do

Replace the home screen · replace the PDF reader · replace notebooks · custom
firmware or kernels · background daemons on the device · an app store · cloud
collaboration · AI summaries · OCR competing with native handwriting search ·
automatic bulk syncing · automatic deletion to save space.

---

## Privacy

No account. No Marginalia server. No telemetry. No analytics. No document or
annotation upload. Everything lives in a local SQLite database on your machine.

The only outbound traffic is to the Zotero API, and only if you configure it.
Credentials live in your operating system's secure storage — macOS Keychain,
Windows Credential Manager, Linux Secret Service — never in the database, never
in a config file, never in a log.

---

## Installing

Full instructions, per platform, with troubleshooting:
**[docs/INSTALL.md](docs/INSTALL.md)**

The short version — you need Rust 1.77+, Node 20+ and pnpm 9+:

```bash
git clone https://github.com/USER/marginalia.git && cd marginalia && pnpm install
```

Verify the core, which needs only Rust:

```bash
cargo test --workspace
```

Run the safety suite:

```bash
cargo test -p marginalia-safety-suite -- --nocapture
```

Launch the app:

```bash
pnpm dev
```

You do not need a reMarkable or a Zotero library to develop Marginalia.
Development runs against a simulator and synthetic fixtures.

---

## Architecture

Desktop-first: roughly 90% of Marginalia runs on your computer, and in V1,
100%. Nothing runs on the reMarkable.

```
apps/desktop/          Tauri 2 shell — React + TypeScript UI, Rust core
packages/core/         domain model, state machines — depends on nothing
packages/safety/       the only place a device write can be authorised
packages/database/     SQLite, migrations, repositories
packages/remarkable/   the device port + firmware compatibility matrix
packages/observability/ structured logging with a SAFETY audit channel
tests/remarkable-simulator/  a simulated device, including its failure modes
tests/safety/          the mandatory safety suite
```

Dependencies point inward. `core` is pure — no filesystem, no network, no
device — which is what makes the safety rules exhaustively testable without
hardware.

**Stack:** Tauri 2, React 18 + TypeScript (strict), Vite, Tailwind, Radix,
TanStack Query, Rust, SQLite + FTS5, `pdfium-render` + `lopdf` for PDF work.

The reasoning behind the stack, including why the PDF layer is Rust rather than
a Python sidecar, is in
[ADR-001](docs/architecture/ADR-001-backend-stack.md).

### Documentation

| Document | What it covers |
|---|---|
| [ARCHITECTURE.md](docs/architecture/ARCHITECTURE.md) | System design and the three invariants |
| [DOMAIN_MODEL.md](docs/architecture/DOMAIN_MODEL.md) | Entities and identity rules |
| [DOCUMENT_STATE_MACHINE.md](docs/architecture/DOCUMENT_STATE_MACHINE.md) | Document lifecycle |
| [SYNC_STATE_MACHINE.md](docs/architecture/SYNC_STATE_MACHINE.md) | Jobs and the transfer firewall |
| [SAFETY_MODEL.md](docs/safety/SAFETY_MODEL.md) | Classification, SafetyManager, mandatory tests |
| [DEVICE_WRITE_POLICY.md](docs/safety/DEVICE_WRITE_POLICY.md) | Exactly what may be written |
| [ZOTERO_SYNC_MODEL.md](docs/zotero/ZOTERO_SYNC_MODEL.md) | How Zotero is used |
| [COMPATIBILITY_MODEL.md](docs/remarkable/COMPATIBILITY_MODEL.md) | Firmware and capabilities |
| [OPEN_QUESTIONS.md](docs/development/OPEN_QUESTIONS.md) | What still needs validating |

---

## Contributing

Contributions are welcome. Because this project touches people's devices and
their research libraries, please read
[CONTRIBUTING.md](CONTRIBUTING.md) and
[SAFETY_MODEL.md](docs/safety/SAFETY_MODEL.md) first.

The short version:

- Never patch `xochitl` or touch system partitions. No flag enables this.
- Metadata sync must never transfer a file.
- Original PDFs are read-only, always.
- Unknown firmware means read-only. Fail closed.
- Every device write goes through `SafetyManager` and carries a `WriteGrant`.
- **If you do not know how a reMarkable behaves, do not guess.** Mark the
  capability `UNKNOWN`, add an entry to
  [OPEN_QUESTIONS.md](docs/development/OPEN_QUESTIONS.md), and stop. Never
  compensate for uncertainty with a more invasive approach.

Good first contributions right now: simulator fixtures, PDF test fixtures,
documentation, UI work on the shell, and — especially — **firmware validation
reports** from real devices, which is what moves capabilities off `UNKNOWN`.

Before opening a pull request:

```bash
pnpm check
```

A pull request with a failing or skipped safety test does not merge.

---

## FAQ

**Will this brick my reMarkable?**
It cannot. Marginalia never writes to system partitions, never patches
`xochitl`, and never touches the bootloader, kernel or update mechanism. The
current build has no device code at all. When transfers do arrive, they are
limited to four whitelisted operations, all verified and reversible.

**Do I need to enable developer mode or install Toltec?**
No Toltec, ever, and never automatically. Reading annotations from the device
will likely require the developer access reMarkable itself provides — that is
tracked as an open question, will be documented, and will always be your
explicit choice.

**Will it fill up my device?**
It cannot. Nothing transfers without you pressing Send on a specific document.
A configurable storage reserve is never spendable, and Marginalia never deletes
anything to make room — it shows you what is large and lets you decide.

**Does it work without Zotero?**
Not yet. Zotero is the bibliographic source of truth in V1. The adapter
boundary is designed so other reference managers are possible later.

**Does it work with reMarkable 1 or Paper Pro?**
V1 targets the reMarkable 2. The device model and capability layer are designed
so other models can be added without redesign, but they are not supported.

**Why is so much of this unfinished?**
Because the alternative is shipping device code before the safety layer that
constrains it. Phase 0 exists so that every later phase is built inside a
structure that makes the dangerous mistakes impossible.

---

## Disclaimer

```
Marginalia is an independent community project.

It is not affiliated with, endorsed by,
or sponsored by reMarkable AS.

reMarkable is a trademark of reMarkable AS.
```

Marginalia uses no official reMarkable logos, branding or assets. Zotero is a
trademark of the Corporation for Digital Scholarship.

## Licence

MIT — see [LICENSE](LICENSE).

The dependency stack was chosen to keep this possible: PDFium is BSD-3 and
`lopdf` is MIT, which is part of why the PDF layer is Rust rather than a
PyMuPDF sidecar (PyMuPDF is AGPL-3.0 or commercial). See
[ADR-001](docs/architecture/ADR-001-backend-stack.md).
