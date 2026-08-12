# Device Write Policy

Status: **Draft v1 — awaiting validation**

The complete, enumerated set of circumstances under which Marginalia is
permitted to change anything on a reMarkable. If an operation is not on this
page, it is forbidden.

---

## 1. The whitelist

Marginalia may write to a reMarkable **only** to:

1. **Add** one user document (PDF) that the user explicitly sent;
2. **Remove** one user document that Marginalia itself transferred, on explicit
   confirmation;
3. **Set native tags** on a document Marginalia manages, from a user-confirmed
   `TagMapping`;
4. **Replace a document Marginalia transferred** with its derived annotated
   version — feature-flagged OFF by default, never for a document we did not
   create.

That is the entire list. Four operations, all user-initiated, all reversible,
all one-document-at-a-time.

## 2. Preconditions for any write

Every one of these must hold. Missing or unevaluable ⇒ denied.

```
[ ] Operation classified YELLOW (never ORANGE under Safe Mode, never RED)
[ ] Feature flag for this operation class is ON
[ ] ExplicitUserIntent token present, fresh, and matches this document
[ ] Device connected, identified, matches the intent's target
[ ] Firmware recognised; capability status == SUPPORTED
[ ] Safe Mode policy permits this class
[ ] Target is inside the user document space
[ ] Target document is either new, or owned by Marginalia per DocumentMapping
[ ] Source PDF opened read-only, structurally validated, checksummed
[ ] Storage projection leaves >= reserve after the write
[ ] SafetySnapshot created and VERIFIED
[ ] Rollback plan constructed and executable
[ ] Idempotency key not already completed
[ ] WriteGrant minted by SafetyManager and not yet consumed
```

## 3. Ownership rule

```
if device_document.uuid not in DocumentMapping.remarkable_document_id
      → it is the user's, not ours
      → read-only, forever, no exceptions
```

Marginalia never modifies, moves, renames, retitles, re-tags, or deletes a
document it did not put there. Notebooks are never written to under any
circumstance.

## 4. Storage policy

- A configurable **reserve** (default 500 MB) is subtracted from free space and
  is never spendable.
- Pre-flight projection is shown before confirmation:
  ```
  Paper size: 84 MB
  Storage after transfer: 2.9 GB available
  ```
- Thresholds: `LOW` warning and `CRITICAL` block, both configurable.
- Marginalia **never** deletes anything to make room. It reports large documents
  and lets the user decide:
  ```
  Large documents on reMarkable
  Machine Learning Book       312 MB
  Research Collection         245 MB
  ```
- No auto-cleanup, no LRU eviction, no "smart" storage management. Ever.

## 5. Transfer pipeline (normative)

```
User clicks Send to reMarkable
      ↓  mint ExplicitUserIntent
Resolve Zotero attachment → local path
      ↓
Open source O_RDONLY · hash · validate PDF structure
      ↓  (any failure → abort, device never contacted)
Check device connection
      ↓
Check firmware capability = SUPPORTED
      ↓
Check storage incl. reserve
      ↓
Create working copy (host temp dir) · checksum
      ↓
Create SafetySnapshot · verify
      ↓
SafetyManager.authorize → WriteGrant
      ↓
Transfer via supported path (one document)
      ↓
Re-read from device · verify checksum · verify listing
      ↓  mismatch → ROLLBACK
Register DocumentMapping · state → ON_REMARKABLE
      ↓
Journal entry · SUCCESS
```

Rollback = remove the partially transferred document **if and only if** it is
identifiable as the one we just created, then restore state and record the
outcome. If rollback cannot be completed safely, we stop, mark the device
read-only, and tell the user exactly what to check. We never "clean up" by
guessing.

## 6. What we never do, restated for contributors

```
✗ write to /usr, /etc, /lib, /bin, /opt, /boot, or any system path
✗ patch, restart-with-modification, or replace xochitl
✗ modify kernel, bootloader, initramfs, or update mechanism
✗ install packages or package managers
✗ create autostart entries or system services
✗ write to notebooks
✗ write to documents we did not transfer
✗ batch-write multiple documents in one operation
✗ delete anything without explicit per-item user confirmation
✗ retry a failed write automatically against the device
✗ "fix" inconsistent device state by writing to it
```

## 7. Contributor checklist

A PR that touches `packages/remarkable` must state, in its description:

1. which of the four whitelisted operations it affects (or "none — read-only");
2. its classification (GREEN / YELLOW);
3. which safety tests cover it;
4. how rollback is tested;
5. simulator fixtures added.

A PR introducing a new device write path that is not one of the four
whitelisted operations requires a change to this document first, reviewed on
its own.
