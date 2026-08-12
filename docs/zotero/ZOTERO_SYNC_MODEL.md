# Zotero Sync Model

Status: **Draft v1 — awaiting validation**

---

## 1. Zotero is the bibliographic source of truth

Zotero owns: item metadata, authors, journals, DOIs, ISBNs, collections,
bibliographic tags, attachments, citation data. Marginalia mirrors that
read-mostly and never tries to become a second Zotero.

Marginalia owns: the reading workflow — device state, transfers, extracted
highlights, notes, reading progress, and the mapping between all of it.

```
Zotero  ──► bibliographic knowledge ──►  Marginalia  ──► reading workflow
        ◄── structured annotations   ◄──              (explicit export only)
```

## 2. The hard rule

> **A Zotero sync never transfers a PDF to the reMarkable.**

Syncing means: metadata, titles, authors, year, DOI, collections, tags,
*attachment availability*, reading state, and Marginalia annotations. It does
not mean downloading or copying files to the device. See
[`SYNC_STATE_MACHINE.md`](../architecture/SYNC_STATE_MACHINE.md) §4 for the
type-level enforcement, and safety tests S8–S10.

"Attachment availability" is a fact we record, not an action we take. Knowing a
PDF exists costs nothing and moves nothing.

## 3. Access strategy

Two adapters behind one `BibliographyProvider` port:

| Adapter | Use | Notes |
|---|---|---|
| **Local Zotero SQLite** (`zotero.sqlite`) | primary for metadata + attachment paths | opened **read-only**, on a *copy* if Zotero holds a lock; never written |
| **Zotero Web API v3** | optional; remote libraries, groups, annotation export | API key in OS secure storage |

We never write to `zotero.sqlite`. Writes to Zotero (annotation/note export)
go through the Web API only, and only on explicit user action.

> ⚠ Uncertainty U4: exact local schema across Zotero 7 versions, and storage
> path resolution (`storage/<key>/<filename>`, linked-file base directories).
> To be validated against a real library before Phase 1 completion.

## 4. Incremental sync algorithm

```
watermark = local last_zotero_version
    ↓
GET /items?since=watermark&limit=100     (paginate; honour Backoff/Retry-After)
    ↓
plan MetadataOperation upserts           (pure; dry-runnable)
    ↓
resolve attachments:  stat(local_path) → availability, size, checksum
                      ── read-only; the file is never copied
    ↓
GET /deleted?since=watermark             → mark deleted_remote
    ↓
commit in ONE transaction; advance watermark LAST
```

Interrupted sync re-runs the last page rather than skipping it. Every operation
is idempotent via `idempotency_key`.

## 5. Attachment availability, not attachment transfer

| Availability | UI | What happens |
|---|---|---|
| `UNKNOWN` | — | resolve on next sync |
| `NOT_PRESENT` | "PDF in Zotero (not downloaded here)" | nothing |
| `AVAILABLE_LOCAL` | "PDF available" + `Send to reMarkable` enabled | nothing until the user clicks |
| `UNREADABLE` | "PDF unreadable" + diagnostics | Send disabled |

The Library must be fully browsable and searchable with **zero** files on the
device.

## 6. Annotation export (Marginalia → Zotero)

Explicit, per-document, never automatic.

| Marginalia | Zotero target |
|---|---|
| `Highlight` with text | annotation item, type `highlight`, with page + position |
| `Highlight` without text | annotation item, type `image`/note fallback, with page |
| `SideNote` | child note (Markdown → HTML), linked to the item |
| `StickyNote` | annotation item, type `note`, with position |

Rules: export is append-preferring; we track `zotero_annotation_key` to avoid
duplicates; re-export updates our own annotations only and never touches
annotations Zotero-side that Marginalia did not create; a failed export leaves
local state untouched and retryable.

## 7. Tag bridge

Zotero tags ↔ Marginalia ↔ reMarkable native tags, via explicit `TagMapping`
rows. Never assume identical semantics; never rename destructively; never apply
an unconfirmed mapping. Conflicts are surfaced:

```
Tag conflict
  Zotero:      machine-learning
  reMarkable:  Machine Learning
  Suggested:   machine-learning ↔ Machine Learning
  [ Confirm ]  [ Keep separate ]
```

## 8. Conflict cases

| Case | Handling |
|---|---|
| Item modified in Zotero *and* annotated locally | independent axes — no conflict; merge |
| Same annotation edited both sides | `CONFLICT`, user review, append-preferring merge |
| Attachment file replaced in Zotero after transfer | checksum mismatch → offer re-send as new generation, keep annotations |
| Item deleted in Zotero | mark `deleted_remote`; **keep** local annotations; surface in UI |
| Two attachments per item | user picks the canonical one for mapping |

## 9. Credentials

API key in OS secure storage (`keyring`). Never in SQLite, config files, logs,
or error messages. Revocable from Settings. No key required at all if the user
only uses the local-database adapter.
