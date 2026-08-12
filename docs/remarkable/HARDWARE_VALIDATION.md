# Hardware validation — reMarkable 2, firmware 3.28.0.166

**Date:** 2026-08-12
**Device:** reMarkable 2.0, Codex Linux 5.8.202, kernel 5.4.70-v1.6.3-rm11x, armv7l
**Release:** 3.28.0.166 (from `/usr/share/remarkable/update.conf`)
**Connection:** Wi-Fi, SSH as `root`
**Documents on device:** 3814 entries in the xochitl store

This is the first time any part of Marginalia has run on a real device. Before
this, every capability claim in the project rested on desk research. What
follows is what was observed, and what changed in the code because of it.

Everything in §1 is read-only. §2 writes, but only inside
`/home/root/.marginalia`, and §3 removes that directory again.

---

## 1. What the device is

| Probe | Result |
|---|---|
| Model | `reMarkable 2.0` (`/proc/device-tree/model`) |
| Kernel | `Linux 5.4.70-v1.6.3-rm11x armv7l` |
| Release | `3.28.0.166` |
| `/etc/version` | `20260806095513` — **a build timestamp, not a version** |
| RAM | 1027 MB total, 750 MB available |
| CPU | 2 cores |
| `/home` | 6.4 GB, 1.1 GB free (82% used) |
| Filesystem | ext4 on `/dev/mmcblk2p4`, `rw,relatime` |
| Coreutils | **BusyBox v1.36.1** |
| Present | `openssl`, `tar`, `gzip` |
| Absent | **`sqlite3`, `curl`, `python3`** |
| CA certificates | 289 in `/etc/ssl/certs` |

Three of these invalidated an assumption in the codebase. They are covered in
§4.

### The absent tools vindicate the single-binary design

There is no `sqlite3`, no `curl` and no `python3` on the device. An agent that
shelled out to any of them would not run. Marginalia compiles SQLite into the
binary and bundles its own TLS roots, so it depends on none of them — a choice
that was made defensively and turns out to have been necessary.

---

## 2. Highlights are stored as text — U3 is answered

This was the project's highest-risk unknown, and the answer is the good one.

`ECOSYSTEM.md` §3 raised the possibility that firmware 3.x had made highlighted
text unrecoverable — the most mature extractor states it does not support ≥ 3.0,
and community tools had resorted to rasterising pages and hunting for
highlight-coloured pixels.

On this device, highlighted text is **not** buried in stroke geometry. It sits
beside the document:

```
<uuid>.highlights/<page-uuid>.json
```

27 such directories were present. The JSON carries, per highlight:

| Field | Meaning |
|---|---|
| `text` | the highlighted text itself |
| `color` | the highlighter colour |
| `start`, `length` | offset into the page's text |
| `rects` | `x`, `y`, `width`, `height` boxes on the page |

Stroke files are `reMarkable .lines file, version=6`.

**What this means for Phase 4.** No OCR. No geometric text-mapping engine. No
intersecting stroke paths with a PDF text layer. Reading a highlight is reading
a JSON file that already contains the text, its colour, its position and its
offset. The "weeks of work" branch that U3 threatened is not the branch we are
on.

**What it does not mean.** `AnnotationRead` stays `UNKNOWN` in the matrix.
Nothing in Marginalia reads these files yet, and a format we have looked at is
not a capability we have implemented and validated. The matrix records the
finding in its notes and grants nothing —
`knowing_the_highlight_format_did_not_by_itself_grant_annotation_reads` in
`compatibility.rs` holds that line.

**Scope of the claim.** One device, one firmware, 27 highlight directories,
inspected structurally. Whether the format is stable across releases, and how
it behaves for EPUB versus PDF, is not established here.

### Privacy note

Highlight text is the user's reading. Only key *names* and value *shapes* were
read during this session; the one `text` value that appeared in output was
redacted at the device before transmission. No document content was copied off
the device, and none is reproduced here.

---

## 3. The agent runs, and removes itself completely

### Install

`./tools/device/install.sh` against the device, over Wi-Fi:

- built for `armv7-unknown-linux-gnueabihf` in a container — 3.9 MB, `ELF
  32-bit LSB pie executable, ARM, EABI5`
- copied to `/home/root/.marginalia/bin/marginalia`, **verified by SHA-256**
  against the local artefact
- manifest written recording version, install time, firmware and checksum
- database created: **schema v2, journal mode `delete`** — the device profile,
  applied on device storage for the first time (bears on U12)

Total footprint: **4.2 MB**.

### Behaviour on unvalidated firmware

The install warned that 3.28.0.166 was not validated, and the agent came up
read-only. `marginalia status` on the device reported:

```
Permitted right now
   no send documents to this device
   no write annotations into PDFs
   no two-way tag sync
```

Fail-closed is not a design intention in a document any more. It was observed
denying every write on real hardware.

### Cost

| Measure | Value | Budget |
|---|---|---|
| Peak RSS (`status`) | **13.2 MB** | < 100 MB |
| Wall time | **0.01 s** | — |
| Resident processes after exit | **0** | — |

The agent is invoke-and-exit. It leaves nothing running.

### Invariants, checked directly

| Check | Result |
|---|---|
| `xochitl` still active | yes, untouched |
| systemd unit named marginalia | none |
| Files outside `/home/root/.marginalia` | none |
| Documents before / after | 3814 / 3814 |
| Highlight directories before / after | 27 / 27 |

### Reset

`./tools/device/reset.sh` listed the three files it would remove, verified every
one was inside `/home/root/.marginalia`, confirmed no system services or startup
entries existed, required the word `remove`, deleted the directory, and then
verified it was gone.

Independently confirmed afterwards: directory `GONE`, 3814 documents present,
27 highlight directories present, `xochitl` active.

The device was returned to stock and Marginalia reinstalled, cleanly, with no
errors.

---

## 4. What hardware changed in the code

Five defects. None would have been found by any test that did not involve a
device.

### 4.1 The firmware version was being read from the wrong file

`tools/device/lib.sh` read `/etc/version`, which on this device holds
`20260806095513` — a build timestamp. The doctor reported that string as the
firmware. Every compatibility statement built on it would have been meaningless.

Fixed to read `REMARKABLE_RELEASE_VERSION` from
`/usr/share/remarkable/update.conf`, falling back to `IMG_VERSION` in
`/etc/os-release`.

### 4.2 The version has four components, not three

`3.28.0.166`. `FirmwareVersion::parse` split on `.`, took three, and dropped the
fourth silently — so two different images compared equal. `build: Option<u32>`
now holds it. Matrix ranges are still `major.minor`, so nothing about capability
lookup widened. Pinned by
`the_four_component_version_a_device_actually_reports_survives_parsing`.

### 4.3 The device's coreutils are BusyBox

`head -30` fails outright: `head: invalid option -- '3'`. The reset script's
file listing was silently producing an error instead of a list. Every remote
`head`/`tail` now uses `-n N`.

### 4.4 The manifest recorded an empty version

Nested quoting in `install.sh` sent a command substitution to the *device*,
where it ran `grep` against a `Cargo.toml` that does not exist and a BusyBox
`cut` that rejected the delimiter — printing `cut: bad delimiter` mid-install
and writing an empty version into the manifest. The version is now read on the
machine that has the checkout, before the SSH call.

### 4.5 `ssh` was eating the operator's answer

`rm_ssh` called `ssh` without `-n`, so ssh consumed the script's stdin and
passed it to the remote command. In `reset.sh`, the safety checks in step 3
swallowed the confirmation intended for step 4, and the removal was refused for
want of an answer that had already been given.

The refusal was correct — it failed closed, which is the whole point — but the
cause was a missing flag. `rm_ssh` now passes `-n`.

One more, not a defect but worth recording: `cross` is installed on the build
machine and does not work there (it cannot provision the pinned toolchain for
the container's architecture). `install.sh` tested for *presence*, chose it, and
aborted. It now falls back to the container build, which needs only Docker.

---

## 5. Questions this settles, and does not

| Question | Before | After |
|---|---|---|
| **U3** — highlight representation | open, severity raised | **answered** — text is stored, as JSON, per page |
| **U12** — journal mode on device storage | open | partially — `delete` mode works on ext4; durability under power loss still unmeasured |
| **U13** — persistence across firmware updates | open | **untouched** — no update happened during this session |
| **U16** — TLS from the device | answered by emulation | unchanged; no network call was made from the device |

`DeviceInfoRead` and `StorageRead` move to `SUPPORTED` for RM2 / 3.28 in the
matrix. They are the only two, because they are the only two whose
implementation is the command that ran. Every write capability remains
`UNKNOWN`, on this firmware and every other.

---

## 6. Reproducing this

```bash
RM_HOST=<device-ip> ./tools/device/doctor.sh
```

Read-only. Reports identity, firmware, free space, and what Marginalia will
never touch.

The device's SSH password is shown under Settings → Help → About → Copyrights
and licenses. Never put it in a file in this repository. If you have shared it,
change it: it is regenerated when developer access is toggled off and on.
