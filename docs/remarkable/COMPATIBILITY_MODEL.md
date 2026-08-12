# reMarkable Compatibility Model

Status: **Draft v1 — awaiting validation**

> **Never guess reMarkable internals.** If current information is uncertain, the
> capability is `UNKNOWN`, which means read-only. Uncertainty is never
> compensated for with a more invasive approach.

---

## 1. Scope

Target: **reMarkable 2**, stock firmware, no Toltec, no developer hacks.
RM1 / Paper Pro are out of scope for V1 but must not be designed out — hence
`Device.kind` and the capability layer.

## 2. Transport candidates

| Transport | Access | Risk | Status |
|---|---|---|---|
| **USB web interface** (`http://10.11.99.1`, user-enabled) | list + upload documents | Low — goes through the device's own app, no filesystem surgery | **Preferred for V1 transfer** — pending validation (U1) |
| **SSH over USB/Wi-Fi** (user-enabled dev access) | read filesystem, read annotation data | Low if strictly read-only; higher if writing | **Read-only use only in V1** (U2) |
| **reMarkable Cloud API** | documents + metadata | Unofficial/unstable; account-coupled | Out of scope for V1 |
| **USB mass storage** | n/a | not offered by the device | n/a |

Both candidate transports require the **user** to enable them on the device.
Marginalia never enables anything on the device itself, and never stores an SSH
password anywhere but OS secure storage.

## 3. What we read, and where from

| Data | Source | Class |
|---|---|---|
| Model, firmware, serial | device info endpoint / `/etc/version`-style read | GREEN |
| Storage total/free | device query | GREEN |
| Document list, uuids, titles, folders | listing endpoint / metadata files | GREEN |
| Native tags | document metadata | GREEN |
| Highlights + handwriting | per-page annotation files (`.rm`, lines format) | GREEN |
| Reading position | document `.content`-style metadata | GREEN |

All of it is copied to the host and parsed on the host. Nothing is parsed,
transformed, or written on the device.

> ⚠ Uncertainty U3: the `.rm` lines format version on current firmware (v6
> family), and the exact highlight representation — whether highlights are
> stored as text-anchored records or as stroke geometry, and whether the device
> exposes selected text directly. This determines how much geometric text
> mapping the Highlight Extractor must do. **Must be validated on a real device
> before Phase 4 implementation begins.**

## 4. Capability matrix

Data, not code: `packages/remarkable/compatibility/matrix.toml`.

```toml
[[entry]]
model = "RM2"; firmware = "3.x"; capability = "MetadataRead"
status = "UNKNOWN"          # ← everything starts UNKNOWN
tested_at = ""              # empty ⇒ loader forces UNKNOWN
method = "usb_web_interface"
notes = "Awaiting first real-device validation."
```

**Loader rule:** an entry with an empty `tested_at` is loaded as `UNKNOWN`
regardless of the `status` value. Optimism in a data file cannot grant
permissions.

Statuses and resolution order: see
[`DEVICE_CAPABILITY_MODEL.md`](../architecture/DEVICE_CAPABILITY_MODEL.md).

## 5. Firmware version handling

- Parse into `(major, minor, patch, build?)`, keep the raw string.
- Match against declared ranges; **no wildcards that silently absorb future
  majors.** `3.x` does not match `4.0.0`.
- A firmware newer than anything in the matrix is `UNKNOWN` → read-only, with a
  clear notice inviting the user to report it.
- A firmware change on a known device invalidates cached `PROBED` capabilities.

## 6. Validation procedure for a new firmware

Required before any matrix entry moves off `UNKNOWN`:

1. Record exact firmware string and device model.
2. Run the read-only probe suite; capture raw responses as simulator fixtures.
3. Add fixtures to `tests/remarkable-simulator/fixtures/`.
4. Set `MetadataRead` / `StorageRead` / `AnnotationRead` to `SUPPORTED` with
   `tested_at`, only if the probes fully succeeded.
5. For `SafeDocumentTransfer`: test in the simulator first; then a real-device
   test with a throwaway PDF, verifying checksum, listing, rollback, and that
   an unrelated document is untouched.
6. Document results in `docs/remarkable/validation/<firmware>.md`.
7. Only then commit the matrix change, with the validation doc in the same PR.

## 7. Never hard-code

```
❌ if firmware == "3.11.2.5" { ... }
❌ assume a path layout
❌ assume an endpoint exists because it did last year
❌ assume highlights carry text
✅ ask the capability layer; degrade gracefully; tell the user why
```

## 8. Firmware updates

Marginalia never disables, delays, blocks, or interferes with device updates.
After an update, the capability layer re-resolves; unknown firmware simply
drops to read-only until validated. A user whose device updates overnight loses
transfer ability temporarily — that is the correct, safe behaviour, and the UI
explains it plainly.
