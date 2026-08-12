# ADR-001 — Backend / local engine stack

- **Status:** Proposed (awaiting validation)
- **Date:** 2026-08-12
- **Deciders:** project lead
- **⚠ Revisit:** this ADR assumed a **desktop** target. The standalone
  reMarkable 2 decision invalidates its central assumption for on-device PDF
  work — there is no obvious prebuilt PDFium for `armv7-unknown-linux-gnueabihf`.
  Recorded as U15. The licensing reasoning below still holds; the binary
  availability reasoning does not.

---

## Context

Marginalia needs a local engine that can: talk to Zotero, talk to a reMarkable
over USB/network, parse reMarkable annotation data, do real PDF work (text
extraction with per-glyph geometry, highlight↔text intersection, writing
annotations into derived copies), run SQLite + FTS5, and ship as a signed
desktop app on macOS, Windows and Linux.

The UI decision is settled: Tauri 2 + React + TypeScript + Vite + Tailwind +
Radix + TanStack Query. The open question is what does the heavy lifting,
specifically PDF processing.

## Options considered

### Option A — Rust only, inside Tauri

PDF work via Rust crates: `pdfium-render` (bindings to Google's PDFium — the
same engine Chrome uses) for text extraction with bounding boxes and rendering;
`lopdf` for low-level PDF object manipulation when writing annotations into a
derived copy.

- ➕ One toolchain, one binary, trivial packaging and code signing.
- ➕ PDFium is battle-tested on adversarial PDFs and gives per-character
  bounding boxes — exactly what highlight↔text mapping needs.
- ➕ Licensing is clean: PDFium is BSD-3, `lopdf` MIT.
- ➕ No IPC boundary, no sidecar lifecycle, no orphaned processes.
- ➖ Requires bundling a PDFium binary per platform (solvable: prebuilt
  binaries, vendored and checksummed).
- ➖ Rust PDF *writing* ergonomics are rougher than PyMuPDF's.

### Option B — Rust + Python sidecar (PyMuPDF)

- ➕ PyMuPDF is the most ergonomic PDF library in existence for this work.
- ➖ **Licensing: PyMuPDF is AGPL-3.0 or commercial.** For a distributed
  desktop app this is a serious constraint that would shape the whole
  project's licensing, even across a process boundary. This alone is close to
  disqualifying for a community-distributed binary.
- ➖ Packaging a Python runtime into a signed, notarised macOS app and a
  Windows installer is a well-known source of pain (PyInstaller + hardened
  runtime + Gatekeeper).
- ➖ Sidecar lifecycle: crashes, zombies, IPC serialisation of large payloads.
- ➖ Doubles the toolchain, CI matrix, and contributor onboarding cost.

### Option C — TypeScript / `pdf.js` in the webview

- ➖ Not viable for the safety-critical path: heavy files in the renderer,
  no clean checksum/atomicity story, weak PDF writing.
- ➕ Fine for *rendering a preview* in the UI, which we may still do.

## Decision

**Option A — Rust only, inside Tauri 2**, with the PDF layer hidden behind a
port so Option B remains available without redesign.

```rust
pub trait PdfEngine {
    fn validate(&self, path: &Path) -> Result<PdfInfo>;
    fn extract_text_with_geometry(&self, path: &Path, page: u32) -> Result<Vec<TextSpan>>;
    fn write_annotations_to_copy(&self, src: &Path, dst: &Path, anns: &[PdfAnnotation]) -> Result<()>;
}
```

`PdfiumEngine` is the V1 implementation. If a case appears that PDFium genuinely
cannot handle, a `SidecarPdfEngine` can be added behind the same trait — and the
licensing question gets faced deliberately at that point, not by default.

Stack summary:

| Concern | Choice |
|---|---|
| Shell | Tauri 2 |
| UI | React 19, TypeScript (strict), Vite, Tailwind, Radix |
| Async state | TanStack Query; Zustand only for genuine global UI state |
| Core | Rust 2021, `tokio` |
| PDF | `pdfium-render` (BSD-3) + `lopdf` (MIT) |
| DB | SQLite via `sqlx` (WAL, FKs on) + FTS5 |
| Types | `ts-rs` generating TS from Rust domain types |
| Secrets | `keyring` → Keychain / Credential Manager / Secret Service |
| Logging | `tracing` + JSON file appender, separate `SAFETY` sink |
| IDs | ULID |

## Consequences

**Positive:** single toolchain; small signed binaries; permissive licensing;
domain logic in the same language as the safety layer, so `WriteGrant` can be
enforced by the type system across the whole backend; simple CI.

**Negative:** writing PDF annotations in Rust is more work than in Python — we
budget extra time in Phase 4 and gate the feature behind
`native_pdf_annotations` (default OFF) until it is proven on the fixture corpus.

**Follow-ups:** vendor + checksum PDFium binaries per platform (ADR-007);
decide the reMarkable transport (ADR-006 — see
[`OPEN_QUESTIONS.md`](../development/OPEN_QUESTIONS.md) U1); choose the project
licence, noting that Option A leaves us free to pick a permissive one.
