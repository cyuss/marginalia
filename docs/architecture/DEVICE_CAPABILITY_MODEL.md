# Device Capability Model

Status: **Draft v1 — awaiting validation**

reMarkable firmware evolves. Marginalia must never hard-code "what a reMarkable
can do". Feature code asks the capability layer; the capability layer answers
from a versioned matrix plus optional read-only probes.

---

## 1. Rule

```
❌  if firmware.starts_with("3.") { do_the_thing() }

✅  if caps.status(Capability::SafeDocumentTransfer) == Supported { ... }
    else { explain_to_user_why_not() }
```

No feature module may parse a firmware string. Only
`packages/remarkable/compatibility` does that.

## 2. Capability keys

| Key | Meaning | Class |
|---|---|---|
| `MetadataRead` | list documents, folders, titles, uuids | GREEN |
| `StorageRead` | total/free bytes | GREEN |
| `DeviceInfoRead` | model, firmware, serial | GREEN |
| `AnnotationRead` | read `.rm` / annotation data | GREEN |
| `NativeTagsRead` | read native tags | GREEN |
| `SafeDocumentTransfer` | upload one PDF via supported path | YELLOW |
| `DocumentRemoval` | delete a document we created | YELLOW |
| `NativeTagsWrite` | set native tags | YELLOW |
| `PdfAnnotationExport` | write derived annotated PDF back | YELLOW (flagged) |
| `CompanionApp` | run a removable companion binary | ORANGE |
| `ExperimentalRmUi` | any UI-side integration | ORANGE |
| `SystemModification` | anything touching system partitions | **RED — never implemented** |

`SystemModification` exists in the enum solely so that the safety layer can
name it and refuse it. There is no implementation behind it.

## 3. Statuses

```
SUPPORTED     verified on this firmware; writes permitted (if YELLOW + flag on)
READ_ONLY     reads verified; writes explicitly withheld
EXPERIMENTAL  behind a feature flag, OFF by default, extra confirmation
UNSUPPORTED   known not to work / known unsafe on this firmware
UNKNOWN       untested firmware → treated as READ_ONLY. Fail closed.
```

**Resolution rule:** `UNKNOWN` never grants a write. Ever. A firmware we have
not tested gets read-only access and a clear UI notice:

```
Unknown firmware detected.

Safe Mode has restricted Marginalia to read-only operations.
No experimental device operations will be performed.
```

## 4. Matrix storage

`packages/remarkable/compatibility/matrix.toml` — data, versioned in git,
separate from feature code:

```toml
[[entry]]
model      = "RM2"
firmware   = "3.x"           # semver range expression
capability = "MetadataRead"
status     = "SUPPORTED"
tested_at  = "TBD"
method     = "usb_web_interface"
notes      = "Verify against a real device before flipping from UNKNOWN."
```

Every entry must record **how** it was verified. An entry with no `tested_at`
is loaded as `UNKNOWN` regardless of what `status` says — the loader enforces
this, so an optimistic edit cannot grant permissions by itself.

## 5. Resolution order

```
1. USER_OVERRIDE    explicit, per-capability, never auto-set,
                    can only DOWNGRADE (Supported→ReadOnly), never upgrade
2. PROBED           read-only probe result from this session
3. MATRIX           versioned table
4. default          UNKNOWN
```

A user override can restrict but not expand permissions. There is no
"I know what I'm doing, enable writes on unknown firmware" switch in V1.

## 6. Probes

Probes are **read-only, side-effect-free, and time-boxed**. Examples: does the
USB web interface respond; does the document listing parse; is the reported
storage plausible. A probe that would require writing to learn the answer is
not a probe — the answer is `UNKNOWN`.

## 7. Presentation

```
reMarkable 2 detected
Firmware 3.xx

✓ Metadata access
✓ Document transfer
✓ Annotation extraction
✓ Zotero sync

⚠ Experimental companion      (off)
✗ UI injection                (not implemented)
✗ System modifications        (never)
```

The last two lines are permanent. They tell the user what Marginalia will never
do, which is as much a feature as what it does.
