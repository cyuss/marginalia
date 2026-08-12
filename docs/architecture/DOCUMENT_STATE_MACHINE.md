# Document State Machine

Status: **Draft v1 — awaiting validation**

Replaces boolean soup (`isDownloaded`, `isSynced`, `hasPdf`) with one explicit
state per document, plus a separate attachment-availability axis.

---

## 1. States

| State | Meaning |
|---|---|
| `METADATA_ONLY` | Zotero knows the item. No PDF resolved on this machine. Nothing on device. |
| `ATTACHMENT_AVAILABLE` | A readable PDF exists locally (Zotero-owned). Still nothing on device. |
| `TRANSFER_PENDING` | User pressed Send. Transfer authorised and in flight. |
| `ON_REMARKABLE` | Transfer verified by checksum. Document present on device, no annotations seen. |
| `ANNOTATED` | Device reports annotation data we have not yet ingested. |
| `CHANGES_PENDING` | Annotations ingested locally; not yet exported to Zotero. |
| `SYNCED` | Device, local store and Zotero agree. Steady state. |
| `CONFLICT` | Divergent changes on two sides, or source file changed under us. |
| `TRANSFER_FAILED` | Transfer aborted and rolled back. Device is clean. |
| `REMOVED_FROM_DEVICE` | User explicitly removed it from the device; annotations retained locally. |

Note there is **no state that means "downloading automatically"**, because no
such operation exists.

## 2. Transition diagram

```
              ┌────────────────────┐
              │   METADATA_ONLY    │◄──────────────┐
              └─────────┬──────────┘               │
              attachment │ resolved                │ attachment lost
                         ▼                         │ / unreadable
              ┌────────────────────┐               │
       ┌─────►│ATTACHMENT_AVAILABLE├───────────────┘
       │      └─────────┬──────────┘
       │                │  ✋ EXPLICIT USER ACTION: "Send to reMarkable"
       │                │     (requires WriteGrant + ExplicitUserIntent)
       │                ▼
       │      ┌────────────────────┐   any precondition or verification failure
       │      │  TRANSFER_PENDING  ├──────────────────────┐
       │      └─────────┬──────────┘                      ▼
       │       checksum │ verified          ┌────────────────────────┐
       │                ▼                   │    TRANSFER_FAILED     │
       │      ┌────────────────────┐        └───────────┬────────────┘
       │      │   ON_REMARKABLE    │             retry  │
       │      └─────────┬──────────┘◄───────────────────┘
       │                │ device reports annotation data
       │                ▼
       │      ┌────────────────────┐
       │      │     ANNOTATED      │
       │      └─────────┬──────────┘
       │                │ ingest + extraction complete
       │                ▼
       │      ┌────────────────────┐   divergent edits   ┌──────────────┐
       │      │  CHANGES_PENDING   ├────────────────────►│   CONFLICT   │
       │      └─────────┬──────────┘                     └──────┬───────┘
       │                │ ✋ explicit "Export to Zotero"         │ resolved
       │                ▼                                       │ by user
       │      ┌────────────────────┐◄──────────────────────────┘
       │      │       SYNCED       │
       │      └─────────┬──────────┘
       │                │ ✋ explicit "Remove from reMarkable"
       │                ▼
       │      ┌────────────────────────┐
       └──────┤  REMOVED_FROM_DEVICE   │   (annotations kept locally)
              └────────────────────────┘

  ✋ = requires an explicit user action. No timer, scheduler, or sync job
       may drive a ✋ transition.
```

## 3. Transition table

| From | Event | To | Guards |
|---|---|---|---|
| `METADATA_ONLY` | attachment resolved readable | `ATTACHMENT_AVAILABLE` | file exists, PDF header valid |
| `ATTACHMENT_AVAILABLE` | attachment unreadable/removed | `METADATA_ONLY` | — |
| `ATTACHMENT_AVAILABLE` | **user: Send** | `TRANSFER_PENDING` | `ExplicitUserIntent` + `WriteGrant`; device connected; firmware `SUPPORTED`; storage incl. reserve; PDF validated; snapshot verified |
| `TRANSFER_PENDING` | destination verified, checksum match | `ON_REMARKABLE` | device checksum == working checksum |
| `TRANSFER_PENDING` | any failure / disconnect / mismatch | `TRANSFER_FAILED` | rollback executed and verified |
| `TRANSFER_FAILED` | **user: Retry** | `TRANSFER_PENDING` | full precondition set re-run |
| `ON_REMARKABLE` | device scan finds annotation data | `ANNOTATED` | mapping matches device UUID |
| `ANNOTATED` | ingest + extraction succeed | `CHANGES_PENDING` | originals untouched |
| `CHANGES_PENDING` | **user: Export to Zotero** | `SYNCED` | export acknowledged by Zotero |
| `CHANGES_PENDING`/`SYNCED` | divergent edits detected | `CONFLICT` | — |
| `CONFLICT` | **user: resolve** | `CHANGES_PENDING` or `SYNCED` | resolution recorded in journal |
| `SYNCED`/`ON_REMARKABLE` | **user: Remove from device** | `REMOVED_FROM_DEVICE` | grant + confirmation; annotations retained |
| `REMOVED_FROM_DEVICE` | **user: Send** again | `TRANSFER_PENDING` | as above |

Any event not in this table is rejected with `IllegalTransition{from, event}`
and logged at `WARN`. Illegal transitions never silently no-op.

## 4. Orthogonal axes (deliberately not folded into the state)

- **AttachmentAvailability** — `UNKNOWN | NOT_PRESENT | AVAILABLE_LOCAL | UNREADABLE`
- **ReadingState.status** — `UNREAD | READING | COMPLETED | ARCHIVED`
- **Annotation counts** — derived, never stored as state

Folding these in would produce a combinatorial explosion. The UI composes them:

```
● On reMarkable   ✎ 18 highlights   ↻ 3 pending   READING
```

## 5. Safety-relevant properties

1. The only edge that writes a file to the device is
   `ATTACHMENT_AVAILABLE --user:Send--> TRANSFER_PENDING`. It is unreachable
   from any sync job, by construction (see ARCHITECTURE §7).
2. `TRANSFER_FAILED` is a *clean* state: reaching it requires a verified
   rollback. If rollback itself fails, the state becomes `CONFLICT` with a
   `SAFETY`-level journal entry and the device is marked read-only until the
   user reviews it.
3. No edge deletes user data. `REMOVED_FROM_DEVICE` removes the device copy only,
   on explicit confirmation, and retains every annotation locally.
