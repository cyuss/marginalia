# Open Questions & Technical Uncertainties

Status: **live document** — every entry must be resolved or explicitly deferred
before the phase that depends on it begins.

Rule: an unresolved uncertainty means the dependent capability stays `UNKNOWN`
(read-only). We never resolve uncertainty by attempting a more invasive method.

---

## U1 — Transfer transport (blocks Phase 3)

**Question.** What is the safest supported way to place a single PDF on a stock
reMarkable 2 on current firmware? The USB web interface upload path is the
leading candidate because it is mediated by the device's own software rather
than by filesystem surgery — but its availability, endpoint shape, and
behaviour on current firmware need first-hand verification.

**Why it matters.** This is the single riskiest operation in the product.

**Resolution.** Read-only probing in Phase 2 on the user's device; document raw
responses; capture as simulator fixtures; write ADR-006. Until resolved:
`SafeDocumentTransfer = UNKNOWN`, Send button disabled with an explanation.

**Never acceptable as a fallback:** writing directly into the device's document
store over SSH and poking `xochitl` to notice. That is filesystem surgery on
live application state.

## U2 — Read access to annotation data (blocks Phase 4)

**Question.** How do we read per-document annotation data? SSH (user-enabled
developer access) appears to be the only route, which means the user must enable
it and supply a password.

**Sub-questions.** Does enabling developer access have any downside for the
user? Does it survive firmware updates? Can we do everything read-only?

**Resolution.** Phase 2, read-only, documented consent flow. Password in OS
secure storage only. `AnnotationRead = UNKNOWN` until validated.

## U3 — Highlight representation in the `.rm` format (blocks Phase 4)

**Question.** On current firmware, are PDF highlights stored as text-anchored
records (with the selected text available) or as stroke geometry we must
intersect with the PDF text layer ourselves? What is the current lines-format
version, and how stable is it?

**Why it matters.** It is the difference between "read the text out" and "build
a geometric text-mapping engine". It changes Phase 4's size by weeks.

**Resolution.** Inspect real annotation files from the user's device on a
throwaway document; build fixtures; then design the extractor. Extraction is
versioned (`extraction_version`) so a format change can be re-run rather than
corrupting stored data.

## U4 — Zotero local database access (blocks Phase 1 completion)

**Question.** Exact `zotero.sqlite` schema on Zotero 7, locking behaviour while
Zotero is running, and storage-path resolution for `imported_file` vs
`linked_file` (including linked-attachment base directories).

**Resolution.** Read-only access, on a copy if locked. Never write. Fall back to
the Web API where the local schema is ambiguous. Validate against the user's
real library early — read-only, so this is safe to do immediately.

## U5 — Native tag read/write (blocks Phase 8 write direction)

**Question.** Where do native tags live in device metadata, and can they be
written without disturbing `xochitl`'s in-memory state?

**Resolution.** Read direction first (GREEN). The write direction stays
`UNKNOWN` until proven; the tag bridge ships read-only if necessary. A
one-directional bridge is a fine V1.

## U6 — Detecting "there are new annotations" cheaply

**Question.** Can we detect changed annotations without pulling every file every
scan (mtime, size, a metadata counter)? Matters for scan latency on a 200-doc
device.

**Resolution.** Measure during Phase 2 probing. Fall back to full-listing +
per-document mtime comparison.

## U7 — PDFium binary distribution

**Question.** Which prebuilt PDFium binaries do we vendor per platform, and how
are they pinned, checksummed, and updated? Any signing/notarisation friction on
macOS?

**Resolution.** ADR-007 before Phase 4. Vendored, checksummed, reproducible.
See also U15: the desktop answer may not survive contact with ARMv7.

## U8 — Writing annotations into derived PDFs with `lopdf`

**Question.** Can we produce standards-compliant highlight/text annotations
(with correct quad points and appearance streams) that open cleanly in Zotero,
Preview, and Acrobat?

**Resolution.** Spike in Phase 4 against the fixture corpus, behind
`native_pdf_annotations` (default OFF). If the output is not clean everywhere,
the feature does not ship — export to Zotero as structured annotations still
works without it.

## U9 — Project licence

**Question.** Which licence? ADR-001 (Rust-only, permissive dependencies) leaves
the choice open; a Python/PyMuPDF path would have forced AGPL considerations.

**Resolution.** User decision. Needed before the first public release, not
before Phase 0.

## U10 — Coordinate systems

**Question.** Reconciling reMarkable page/canvas coordinates with PDF user space
(origin, units, rotation, cropbox vs mediabox, scaling of a PDF page onto the
device canvas).

**Resolution.** Design note + property-based tests in Phase 4:
`device → pdf → device` must round-trip within tolerance on rotated, cropped,
and non-A4 pages.

---

## Resolution log

| ID | Status | Resolved | ADR |
|---|---|---|---|
| U1 | open | — | ADR-006 (pending) |
| U2 | open | — | — |
| U3 | open | — | — |
| U4 | open | — | — |
| U5 | open | — | — |
| U6 | open | — | — |
| U7 | open | — | ADR-007 (pending) |
| U8 | open | — | — |
| U9 | open | — | — |
| U10 | open | — | — |

---

## Standalone-reMarkable unknowns (added 2026-08-12)

Raised by the Phase 0.5 audit. See
[`../migration/phase-0-to-standalone-rm2.md`](../migration/phase-0-to-standalone-rm2.md).

## U11 — Display access for a third-party app on RM2 (BLOCKS PHASE 1)

**Question.** How can an application that is not `xochitl` draw to the screen
without patching or injecting into `xochitl`, replacing system libraries, or
installing a system package manager?

**Why it matters.** It determines whether the product has an on-device UI at
all. Every route currently known to me collides with a safety invariant; a
fourth option (no custom UI, native reader as the interface) avoids the problem
entirely but changes the product shape.

**Resolution.** Documented read-only hardware probe, then
[ADR-002](../adr/ADR-002-remarkable-ui-and-runtime.md). **Do not resolve this
by picking the more invasive option.**

## U12 — SQLite journal mode on the device filesystem

Whether WAL is safe and performant on the device's storage, and whether
`synchronous = FULL` is warranted given unannounced power loss. Until measured,
the device profile defaults to the durable option. See
[ADR-005](../adr/ADR-005-device-storage-profile.md).

## U13 — Application persistence across firmware updates

Whether a manifest-owned install survives a firmware update, and what the
uninstall/rollback story is when it does not.

## U14 — Logging cost on the device

Runtime and binary-size cost of `tracing-subscriber` with the JSON formatter on
an ARMv7 device. A lighter sink may be required.

## U15 — PDF text and geometry extraction within RM2 budgets

ADR-001 chose `pdfium-render` for a **desktop** target. There is no obvious
prebuilt PDFium binary for `armv7-unknown-linux-gnueabihf`, and PDFium is large.
Blocks Phase 5. Options to evaluate: build PDFium for armv7, a pure-Rust
extractor, or extraction on the desktop companion only — the last of which would
violate the standalone requirement for that feature and must be called out as
such rather than quietly accepted.

## U16 — TLS on the device

Which root certificate store is available, whether to bundle one, and how it is
updated. Blocks Phase 2 (direct HTTPS to the Zotero API from the device).

**Partially answered 2026-08-12.** Building the Zotero HTTP client produced two
concrete findings:

1. **`ureq`'s TLS chain needs a cross C compiler for armv7**, failing with
   `failed to find tool "arm-linux-gnueabihf-gcc"` — the *same* blocker as
   `libsqlite3-sys` (U17). One cross toolchain therefore unblocks both. This is
   a build-environment problem, not an architecture one.
2. **It raised the project MSRV to 1.90.** `ureq` → `url` → `idna` → `icu_*`
   requires rustc 1.86+. Pinning around it was attempted and does not work: the
   whole `idna`/`url` subtree moved together. `rust-toolchain.toml` was raised
   from 1.85 to 1.90 and records why.

Still open: which root store the device has, and whether we bundle one. The
`http` feature is **off by default** so the portable crates keep cross-compiling
while this is unresolved.

## U17 — Cross-compiling the C dependencies

`marginalia-database` (via `libsqlite3-sys`) and the Zotero `http` feature (via
its TLS stack) both need a cross C toolchain. Not an architecture problem; see
[ADR-003](../adr/ADR-003-cross-compilation.md).

Severity raised from low to **medium**: it now blocks two crates rather than
one, and it is on the critical path for any real device build. The remedy is
unchanged and well understood — `cross`/Docker, or a host cross-gcc.

---

## Resolution log (standalone)

| ID | Status | Severity | Blocks |
|---|---|---|---|
| U11 | open | **blocking** | Phase 1, all UI |
| U12 | open | high | Phase 1 storage |
| U13 | open | high | Phase 1 packaging |
| U14 | open | medium | Phase 1 budgets |
| U15 | open | high | Phase 5 |
| U16 | partially answered | medium | Phase 2 |
| U17 | open | **medium** | build slice 8; blocks database + TLS |
