# reMarkable Simulator — Specification

Status: **Draft v1 — awaiting validation**

> The physical reMarkable 2 must **not** be required for development. Every
> device-touching code path must be exercisable, including its failure modes,
> without a device in the room.

---

## 1. Purpose

The simulator implements the `DeviceProvider` port in-process, so the whole
stack above it — SafetyManager, transfer pipeline, ingest, state machines — runs
identically against it. It exists to make dangerous things testable:
disconnection mid-write, checksum mismatch, low storage, unknown firmware.

## 2. Design

```rust
pub struct SimulatedDevice {
    profile: DeviceProfile,        // model, firmware, storage, capabilities
    documents: BTreeMap<Uuid, SimDocument>,
    script: FaultScript,           // deterministic, seeded fault injection
    journal: Vec<SimEvent>,        // every call recorded for assertions
}
impl DeviceProvider for SimulatedDevice { /* ... */ }
```

Properties: **deterministic** (seeded, no wall-clock dependence), **observable**
(full call journal), **assertive** (it panics the test if an invariant is
violated, e.g. a write arriving without a grant), and **stateful across a test**
so idempotency can be verified.

## 3. Device profiles

| Profile | Firmware | Storage free | Purpose |
|---|---|---|---|
| `rm2_known_healthy` | known, `SUPPORTED` | 3.1 GB | happy path |
| `rm2_unknown_firmware` | `9.9.9` | 3.1 GB | S1: writes denied |
| `rm2_low_storage` | known | 400 MB (< reserve) | S2 |
| `rm2_critical_storage` | known | 20 MB | threshold UI |
| `rm2_read_only_matrix` | known, `READ_ONLY` | 3.1 GB | capability downgrade |
| `rm2_populated` | known | 1.2 GB | 200 docs incl. foreign ones |
| `rm2_disconnecting` | known | 3.1 GB | S5 |
| `rm2_flaky` | known | 3.1 GB | retries, backoff |
| `rm2_corrupting` | known | 3.1 GB | S4: silently alters bytes |
| `rm2_slow` | known | 3.1 GB | timeouts, progress UI |

## 4. Fault script

Deterministic injection, addressed by call index or byte offset:

```toml
[[fault]]
at = "upload_document"
occurrence = 1
after_bytes = 4_194_304
kind = "connection_lost"
```

Fault kinds: `connection_lost`, `truncated_write`, `checksum_mismatch`,
`storage_shrinks_mid_operation`, `listing_omits_uploaded_doc`,
`duplicate_uuid_returned`, `slow_response{ms}`, `http_5xx`, `permission_denied`,
`firmware_changes_mid_session`, `rollback_fails`.

`rollback_fails` exists specifically to test S14 — that we degrade to read-only
and raise a blocking notice rather than improvising.

## 5. Built-in invariant assertions

The simulator fails the test immediately if:

1. a write arrives without a valid, unconsumed `WriteGrant`;
2. a write targets a document not in `DocumentMapping` (foreign document);
3. a write targets anything outside the user document space;
4. a delete occurs without an `ExplicitUserIntent`;
5. more than one document is written in a single operation;
6. **any** file transfer occurs during a `ZOTERO_METADATA` job (S8);
7. a system path is touched (RED) — this must be unreachable, and the
   simulator proves it stays unreachable.

## 6. PDF and annotation fixtures

`tests/fixtures/` — all synthetic and sanitised. **No production Zotero library
is ever used as a fixture.**

```
pdf/no_annotations.pdf          pdf/with_highlights.pdf
pdf/with_handwriting.pdf        pdf/mixed_annotations.pdf
pdf/large_300mb.pdf (generated) pdf/malformed_header.pdf
pdf/encrypted.pdf               pdf/zero_pages.pdf
pdf/scanned_no_text_layer.pdf   pdf/rtl_and_cjk.pdf
pdf/rotated_pages.pdf           pdf/two_column.pdf

zotero/library_small.json       zotero/duplicate_attachment.json
zotero/missing_attachment.json  zotero/tag_conflicts.json
zotero/deleted_items.json       zotero/group_library.json

annotations/rm_v6_highlight_simple/    annotations/rm_v6_highlight_multiline/
annotations/rm_v6_handwriting/         annotations/rm_v6_mixed/
annotations/rm_truncated_file/         annotations/rm_unknown_version/
```

`rm_unknown_version` must be handled as "unparseable, report honestly" — never
as a best-effort guess that could produce wrong highlight text.

> ⚠ The `annotations/rm_v6_*` fixtures cannot be authored accurately until
> uncertainty U3 is resolved on a real device. Until then they are placeholders
> and the Highlight Extractor is not implemented. See
> [`OPEN_QUESTIONS.md`](../../docs/development/OPEN_QUESTIONS.md).

## 7. Scenario tests

Each mandatory safety test S1–S15 has a named simulator scenario. Additional
scenarios: duplicate `Send` (idempotency), interrupted then resumed sync,
firmware upgrade between sessions, 200-document listing performance,
foreign-document protection, storage projection accuracy.

## 8. Real-device policy

A real reMarkable is used only in **Phase 2 validation** (read-only) and
**Phase 3 validation** (one throwaway PDF), after the full simulator suite is
green — and always with a device the user is willing to factory-reset.
