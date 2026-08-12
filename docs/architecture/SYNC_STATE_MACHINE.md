# Sync State Machine

Status: **Draft v1 — awaiting validation**

Covers the lifecycle of a `SyncJob` and the hard separation between metadata
work and file transfer.

---

## 1. Job kinds

| Kind | Direction | Touches device files? | Trigger |
|---|---|---|---|
| `ZOTERO_METADATA` | Zotero → Marginalia | **Never** | user / schedule / startup |
| `DEVICE_SCAN` | Device → Marginalia | Read-only | user / schedule |
| `ANNOTATION_INGEST` | Device → Marginalia | Read-only | user / after scan |
| `TRANSFER` | Marginalia → Device | **Yes — write** | **user only** |
| `REMOVAL` | Marginalia → Device | **Yes — delete** | **user only, confirmed** |
| `ZOTERO_EXPORT` | Marginalia → Zotero | No | **user only** |
| `TAG_BRIDGE` | bidirectional | Metadata only | user, confirmed mappings only |

## 2. Job lifecycle

```
   CREATED
      │  planner runs
      ▼
   PLANNED ────────────────► REJECTED      (preconditions failed; nothing done)
      │  authorised
      ▼
   RUNNING ────────────────► CANCELLING ──► CANCELLED
      │                                          │
      ├──► COMPLETED                             │ (partial ops rolled back)
      ├──► COMPLETED_WITH_WARNINGS               │
      └──► FAILED ──► ROLLING_BACK ──► ROLLED_BACK
                              │
                              └──► ROLLBACK_FAILED  ⚠ SAFETY event,
                                   device marked READ_ONLY until user review
```

`ROLLBACK_FAILED` is the only terminal state that degrades device permissions.
It is loud: `SAFETY` log, journal entry, blocking UI notice.

## 3. Planner / executor split

```
   ┌──────────────┐    reads only     ┌──────────────┐
   │ SyncPlanner  │ ────────────────► │   SyncPlan   │
   └──────────────┘                   └──────┬───────┘
        no side effects                      │  authorised by SafetyManager
        fully unit-testable                  ▼
                                      ┌──────────────┐
                                      │ SyncExecutor │  ── side effects here only
                                      └──────┬───────┘
                                             ▼
                                       SyncJournal
```

The planner is a pure function of (local state, remote state, config) → plan.
Every plan can be **dry-run**: rendered to the user as a list of intended
operations before anything executes.

## 4. The type-level firewall (INV-2)

```rust
enum MetadataOperation {           // ZOTERO_METADATA, TAG_BRIDGE, DEVICE_SCAN
    UpsertItem(ZoteroItem),
    UpsertCollection(ZoteroCollection),
    UpsertAttachmentAvailability { key: String, availability: AttachmentAvailability },
    UpsertTag(Tag),
    LinkDeviceDocument { mapping: DocumentMappingId, device_uuid: String },
    RecordAnnotationMetadata(AnnotationMeta),
    // ← there is deliberately NO variant that can move a file.
}

enum TransferOperation {           // TRANSFER, REMOVAL
    UploadPdf { grant: WriteGrant, intent: ExplicitUserIntent, source: ValidatedPdf },
    RemoveDeviceDocument { grant: WriteGrant, intent: ExplicitUserIntent, uuid: String },
}

impl SyncExecutor  { fn execute(&self, ops: Vec<MetadataOperation>) -> ... }
impl TransferExecutor { fn execute(&self, ops: Vec<TransferOperation>) -> ... }
```

`ExplicitUserIntent` is minted only by the command handler bound to the Send /
Remove buttons, carries the document id and a user-confirmation timestamp, and
is consumed on use. A scheduler has no way to produce one.

**Consequence:** "metadata sync accidentally transferred my PDFs" is not a bug
that can be introduced by a careless change — it requires deleting a type.

## 5. Incremental Zotero sync

```
  read local zotero_version watermark
        │
        ▼
  GET /items?since=<version>&limit=100   ── paginate via Link headers
        │                                   respect Backoff / Retry-After
        ▼
  for each page:  plan upserts (metadata only)
        │
        ▼
  resolve attachment availability locally  ── stat() the file, hash it
        │                                     NEVER copy it anywhere
        ▼
  detect deletions via /deleted?since=<version>
        │
        ▼
  commit in ONE transaction, advance watermark last
```

Watermark advances only after a successful commit, so an interrupted sync
re-runs the same page rather than skipping it. All operations are idempotent
(`idempotency_key = kind + target_ref + content_hash`).

Rate limiting: honour `Backoff` and `Retry-After` headers; exponential backoff
with jitter on 5xx; hard stop after N attempts with a resumable job.

## 6. Sync result reporting

Every job produces counters, and the UI **always shows the transfer count**,
precisely so the user can see it is zero:

```
Sync completed

Updated metadata        12
New Zotero items         4
Updated tags             7
Annotations imported     8
PDFs transferred         0        ← always displayed, always 0 for metadata sync

Duration              3.2s
```

## 7. Conflict handling

Detected in the planner, never resolved silently:

| Conflict | Detection | Default proposal |
|---|---|---|
| Annotations changed on device *and* in Zotero | both sides' modified > last_synced | merge non-conflicting, ask on overlaps |
| Zotero source file changed after transfer | `original_checksum` mismatch | offer re-send as new generation; keep annotations |
| Same Zotero item mapped to two device docs | mapping uniqueness violated | ask which is canonical |
| Tag name collision under normalisation | normalised equality, raw inequality | propose mapping, require confirmation |

Merge semantics are **append-preferring**. Replacement requires explicit choice.
Nothing is deleted to resolve a conflict.

## 8. Journal

Every `SyncOperation` writes a journal row. The Activity view renders them
human-readably with technical detail on demand:

```
14:32   Zotero metadata synced          12 items, 0 PDFs transferred
14:28   8 annotations imported          Attention Is All You Need
14:15   Sent to reMarkable              Attention Is All You Need · 12.4 MB · verified
13:42   Tag mapping updated             machine-learning ↔ Machine Learning
```
