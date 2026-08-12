# Marginalia — Safety Model

Status: **Draft v1 — awaiting validation**

> No feature is important enough to risk making the reMarkable unusable.
> When choosing between *more integrated but risky* and *less integrated but
> safe*, Marginalia always chooses **safe**.

---

## 1. Threat model — what we are protecting against

Not attackers. **Ourselves.** The realistic failure modes are:

| # | Risk | Mitigation |
|---|---|---|
| R1 | A write corrupts `xochitl` state or a notebook | Only supported, xochitl-mediated write paths; no direct filesystem surgery in V1 |
| R2 | Interrupted transfer leaves a half-file | Staged upload + post-verify + rollback; never in-place |
| R3 | Untested firmware behaves differently | `UNKNOWN` → read-only, fail closed |
| R4 | Device storage exhaustion | Hard reserve + pre-flight projection + refusal |
| R5 | Original Zotero PDF damaged | Sources opened `O_RDONLY`; all mutation on copies |
| R6 | Bulk accidental transfer | Type-level firewall (INV-2) + per-document explicit intent |
| R7 | Silent data loss on conflict | No destructive resolution; append-preferring merge |
| R8 | A future contributor adds an unguarded write | `WriteGrant` required by signature; CI grep + arch tests |
| R9 | Marginalia breaks a firmware update | We never touch update mechanisms or system partitions |
| R10 | User can't recover | Snapshots, journal, and full reversibility of every YELLOW op |

## 2. Classification of every device-touching operation

**GREEN** — read-only, no device state change.
**YELLOW** — controlled, reversible, verified write, user-initiated.
**ORANGE** — experimental integration, flag-gated OFF, removable.
**RED** — system modification. **Never implemented. Not behind a flag. Not at all.**

### GREEN — permitted always (Safe Mode included)

| Operation | Notes |
|---|---|
| Detect device presence / connection | USB or network probe |
| Read model + firmware version | |
| Read serial (stored hashed) | |
| Read total / free storage | |
| List documents, folders, uuids, titles | |
| Read document metadata JSON | |
| Read native tags | |
| Read annotation / `.rm` data | copied to host, parsed on host |
| Read page thumbnails/previews | |
| Read reading position if exposed | |
| Compute checksums of read data | |

### YELLOW — permitted only with grant + explicit user intent

| Operation | Required gates |
|---|---|
| Upload one validated PDF (`Send to reMarkable`) | flag ON, firmware `SUPPORTED`, storage + reserve, PDF validated, snapshot verified, checksum verified post-transfer, rollback ready |
| Remove a document **we** transferred | explicit confirm dialog, mapping-verified uuid, snapshot |
| Write native tags on a document we manage | capability `SUPPORTED`, confirmed TagMapping |
| Write a derived annotated PDF back to device | feature flag `native_pdf_annotations` (default OFF), original untouched |
| Create a host-side backup/snapshot of device data | read on device, write on host only |

Constraints binding every YELLOW operation:
- one document per operation — no batch device writes in V1;
- never overwrites a document Marginalia did not create;
- never touches anything outside the user document space;
- verified after the fact, rolled back on any mismatch;
- fully reversible by the user.

### ORANGE — Phase 10 only, flag OFF, not in V1

| Operation | Notes |
|---|---|
| Install a removable companion binary | isolated, user-space, uninstallable, no autostart in system units |
| Companion command palette | must not override native gestures |
| Experimental non-system overlay | requires prior real-device validation + documented rollback |

ORANGE work does not begin until the desktop app is stable and a written
risk assessment exists per operation.

### RED — prohibited, permanently

```
Patch or replace xochitl
Modify /usr, /etc, /lib, /bin, or any system partition
Modify bootloader or kernel
Replace or shim system libraries
Disable, defer, or interfere with firmware updates
Install Toltec (or any package manager) automatically
Modify system configuration or systemd units
Delete user documents automatically
Overwrite an original PDF
Write to documents Marginalia did not create
Any undocumented system hack
```

There is no flag, no advanced setting, and no debug mode that enables a RED
operation. The enum variant `Capability::SystemModification` exists only so the
safety layer can refuse it by name.

## 3. SafetyManager

Single choke point. Everything device-related passes through it.

```rust
pub enum Authorization {
    Granted(WriteGrant),
    Denied { reason: DenialReason, user_message: String, remediation: Option<String> },
}

impl SafetyManager {
    pub fn authorize(&self, req: OperationRequest) -> Authorization;
}
```

Evaluation order — **first failure denies**, and denial is the default if any
step cannot be evaluated:

1. **Classification** — RED → immediate hard denial, `SAFETY` log.
2. **Feature flag** — off → denied.
3. **Device identity** — device present, identified, matches request target.
4. **Firmware & capability** — matrix status must be `SUPPORTED`
   (`UNKNOWN`/`READ_ONLY`/`UNSUPPORTED`/`EXPERIMENTAL`-without-flag → denied).
5. **Safe Mode policy** — Safe Mode ON restricts to GREEN + the explicitly
   allowed YELLOW set; ORANGE always denied under Safe Mode.
6. **Preconditions** — storage projection incl. reserve, PDF structural
   validation, source checksum, mapping consistency, idempotency check.
7. **Snapshot** — created *and verified* where the operation class requires it;
   unverifiable snapshot ⇒ treated as no snapshot ⇒ denied.
8. **Rollback plan** — must exist and be executable; otherwise denied.

Only then is a `WriteGrant` minted: single-use, operation-scoped,
device-scoped, TTL-bounded, and consumed by the executor.

### Fail-closed guarantee

Any error, timeout, parse failure, ambiguity, or unhandled case inside
`authorize` returns `Denied`. There is no `_ => Granted` anywhere. An
architecture test asserts this by scanning for a `Granted` construction outside
the single authorised code path.

## 4. Safe Mode

**Default: ON.** Persisted per device.

| | Safe Mode ON | Safe Mode OFF (advanced) |
|---|---|---|
| GREEN | allowed | allowed |
| YELLOW | allowed for the vetted set, one doc at a time | same + fewer confirmations |
| ORANGE | **denied** | flag-gated, extra confirmation |
| RED | denied | **denied** |

Safe Mode OFF never unlocks RED and never unlocks writes on `UNKNOWN` firmware.
It is a convenience reduction, not a safety bypass.

## 5. Atomicity model

```
READ (source, O_RDONLY)
  ↓
COPY to working directory
  ↓
MODIFY the copy
  ↓
VALIDATE (structural PDF check, page count, renderability)
  ↓
CHECKSUM
  ↓
STAGE on destination
  ↓
VERIFY on destination (re-read, re-checksum)
  ↓
COMMIT (register mapping, advance state)
```

Never `READ → MODIFY ORIGINAL`. If any step fails: discard the working copy,
execute rollback, record a journal entry, surface a truthful error.

## 6. Snapshots

`SafetySnapshot` records, before a YELLOW operation: affected document ids,
their checksums, device storage figures, and the operation descriptor. Status
must reach `VERIFIED` before the operation proceeds. Snapshots are host-side;
we never write backup files to the device.

## 7. Logging

`SAFETY` is a first-class level with its own persisted, user-viewable audit
trail. Every authorization decision (granted **and** denied), every rollback,
and every capability downgrade is recorded. Never logged: Zotero keys, tokens,
credentials, or note contents (sanitised context only, in debug builds).

## 8. Definition of safe

A feature ships only when all of these hold:

```
✓ removing Marginalia leaves the reMarkable fully usable
✓ an app crash mid-operation leaves the reMarkable fully usable
✓ an interrupted transfer cannot corrupt existing device data
✓ the original document remains byte-identical and recoverable
✓ unsupported/unknown firmware prevents the dangerous write
✓ the operation is reproducible end-to-end in the simulator
✓ rollback is implemented AND tested, including rollback failure
```

## 9. Mandatory safety tests

Each maps to a test in `tests/safety/`. None may be skipped or marked flaky.

| ID | Assertion |
|---|---|
| S1 | unknown firmware → all writes denied |
| S2 | insufficient storage (incl. reserve) → transfer denied |
| S3 | invalid/corrupt PDF → transfer denied before any device contact |
| S4 | checksum mismatch after transfer → rollback executed, state `TRANSFER_FAILED` |
| S5 | connection lost mid-transfer → no partial commit, no mapping registered |
| S6 | annotation merge failure → original untouched, derived copy discarded |
| S7 | duplicate `Send` → idempotent, exactly one device document |
| S8 | metadata sync → **zero** device PDF transfers |
| S9 | Zotero item with PDF attachment → metadata sync only, file never read for copy |
| S10 | user clicks Send → exactly one transfer, exactly one document |
| S11 | failed Send → Zotero source file byte-identical (hash before/after) |
| S12 | experimental feature disabled → no experimental device operation reachable |
| S13 | RED operation requested programmatically → hard denial + `SAFETY` log |
| S14 | rollback failure → device marked read-only, blocking notice raised |
| S15 | device document not in our mappings → never modified or deleted |
