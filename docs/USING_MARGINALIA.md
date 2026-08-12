# Using Marginalia on your reMarkable

What you can actually do today, what is coming, and one thing that is not
coming — with the reason.

Everything here was run against a reMarkable 2 on firmware 3.28.0.166.

---

## First, the honest part: there is no Marginalia app on the screen

If you are picturing a Marginalia window on the reMarkable — a split view, a
sidebar, a button, a menu — that is not what this is, and it is not a feature
waiting to be built. It is ruled out.

**Why.** reMarkable publishes no supported way for a third-party application to
draw to the display. The community route, `rm2fb`, works by `LD_PRELOAD`-ing a
server into a system binary and a shim into every application. That means
modifying the device's own software, which is the first thing this project
promises never to do. The full reasoning is in
[ADR-002](adr/ADR-002-remarkable-ui-and-runtime.md); the evidence is in
[ECOSYSTEM.md](remarkable/ECOSYSTEM.md) §2.

So a split view is not on the roadmap. Not "later" — the only known way to
build it requires breaking the guarantee that makes this project worth
installing.

**What you get instead.** The reMarkable's own reader is the interface. You read
and highlight exactly as you do now, with nothing in the way. Marginalia works
behind it: it reads what the device already stored, and — in later phases —
writes documents *into your library* that you open in the native reader like
any other. A generated index you tick with the stylus is how you will ask it for
things ([ADR-006](adr/ADR-006-on-device-interaction.md)).

That constraint turns out to be a decent product. Nothing to learn, nothing to
launch, nothing that can crash your reader.

---

## What works today

### Install

From your computer, with the reMarkable on the same Wi-Fi (or over USB):

```bash
RM_HOST=<your-device-ip> make device-install
```

Find the IP and the SSH password under **Settings → Help → About → Copyrights
and licenses**. Check first, changing nothing:

```bash
RM_HOST=<your-device-ip> make device-doctor
```

Everything Marginalia places lives in `/home/root/.marginalia`. Removing that
directory removes all of it — see [Uninstalling](#uninstalling).

### Read back everything you have highlighted

This is the feature that works today, and it is the one that changes how you
read.

Highlight text in the reMarkable's own reader — select with the stylus, choose
the highlighter. Then:

```bash
ssh root@<your-device-ip> '/home/root/.marginalia/bin/marginalia highlights'
```

```
26 document(s), 2624 highlight(s)

  Albano et al. - 2015 - Relational DBMS Internals       200  pdf
  An introduction to Science                             201  pdf
  Cultures and Organizations Software for the Mind, T…   546  pdf
  Downey - 2012 - Think Complexity                        77  pdf
  God and Time                                            11  pdf
  …
```

The passages themselves, by title fragment:

```bash
ssh root@<ip> '/home/root/.marginalia/bin/marginalia highlights "Think Complexity"'
```

```
Downey - 2012 - Think Complexity
───────────────────────────────

  a graph is a set of nodes and a set of edges
      — page 12
```

As Markdown, one file per document:

```bash
ssh root@<ip> '/home/root/.marginalia/bin/marginalia highlights --export'
scp -r root@<ip>:/home/root/.marginalia/highlights/ .
```

Each file is a blockquote per passage with its page number, ready to paste into
Obsidian, Zotero notes, or anything else.

As JSON, for scripting:

```bash
ssh root@<ip> '/home/root/.marginalia/bin/marginalia highlights --document <uuid>'
```

**This only ever reads your library.** It does not modify, move or re-save a
single document. The exported Markdown is written inside Marginalia's own
directory, never into your reMarkable's collections.

### Check on it

```bash
ssh root@<ip> '/home/root/.marginalia/bin/marginalia status'
ssh root@<ip> '/home/root/.marginalia/bin/marginalia doctor'
```

`status` tells you what Marginalia is *permitted* to do right now. On firmware
it has not been validated against, the answer is: read, and nothing else.

### Connect Zotero (optional)

Zotero is one library source among several — not what the tool is for. If you
use it:

```bash
ssh root@<ip> '/home/root/.marginalia/bin/marginalia zotero connect <api-key>'
ssh root@<ip> '/home/root/.marginalia/bin/marginalia sync'
```

Sync brings titles, authors, collections and tags. **It never transfers a PDF.**
Moving a document to your device is always a separate, explicit request.

If you do not use Zotero, a plain folder of documents works as a library source
instead, and nothing about the rest of the tool changes.

---

## Uninstalling

```bash
RM_HOST=<your-device-ip> make device-reset
```

It lists every file it will remove, verifies each one is inside
`/home/root/.marginalia`, checks no system service or startup entry exists, asks
you to type `remove`, deletes, and then verifies the directory is gone.

There is nothing to restore afterwards, because nothing was replaced. Your
notebooks, documents and reading positions are untouched — this has been run on
a device with 3814 documents and verified before and after.

---

## What is coming

| | |
|---|---|
| Highlights into a database, with history | Phase 4 |
| Reading digests generated as documents in your library | Phase 6 |
| Asking for things by ticking a generated index with the stylus | Phase 7 ([ADR-006](adr/ADR-006-on-device-interaction.md)) |
| Sending a document to the device, explicitly | Phase 3 |
| Handwritten margin notes, not just highlights | needs the `.rm` v6 stroke parser |
| Tag bridging with the device's own tags | Phase 8 |

Handwritten notes are a harder problem than highlights: highlighted text is
stored as text, but handwriting is stroke geometry. It will arrive as strokes
first and text later, if at all.

---

## What it will never do

Not "not yet" — never:

- modify `xochitl`, the kernel, the bootloader or any system partition
- install a package manager
- create a startup entry or a system service
- delete one of your documents, or free space by removing anything
- modify an original PDF
- transfer a file anywhere without you asking for that transfer

If a future version needs to cross one of these lines, it will not be this
project.

---

## Troubleshooting

**`Could not read the reMarkable's document store`** — the `highlights` command
reads the device's own library, so it runs *on* the reMarkable, over SSH. Run it
as shown above rather than on your computer.

**`This firmware has not been validated`** — expected, and not a failure. No
firmware has been validated for writes yet. The agent runs read-only, which is
everything the current features need.

**`N document(s) could not be read`** — listed rather than skipped, on purpose.
The reason is printed for each. Nothing was changed; please open an issue with
the message.

**The build fails with SIGBUS inside rustc** — this was the container build
memory-mapping compiler artefacts across the macOS bind mount, and it is fixed:
the ARM target tree now lives in a Docker volume. If you still see it, clear the
cache and rebuild:

```bash
docker volume rm marginalia-armv7-target
```

---

## Where the guarantees are written down

- [SAFETY_MODEL.md](safety/SAFETY_MODEL.md) — what protects your device
- [DEVICE_WRITE_POLICY.md](safety/DEVICE_WRITE_POLICY.md) — exactly what may be written
- [HARDWARE_VALIDATION.md](remarkable/HARDWARE_VALIDATION.md) — what a real device did, measured
- [OPEN_QUESTIONS.md](development/OPEN_QUESTIONS.md) — what is still unknown
