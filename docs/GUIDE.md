# Marginalia — install it, and use it

A complete walkthrough, start to finish. Every command here was run against a
reMarkable 2 on firmware **3.28.0.166** with 3,814 documents on it, and the
output shown is the output it produced.

Follow it top to bottom and you will end with your own highlights on your own
computer, and a device you can return to stock in one command.

- [Before you start](#before-you-start)
- [1 · Get the code](#1--get-the-code)
- [2 · Find your reMarkable](#2--find-your-remarkable)
- [3 · Check, before changing anything](#3--check-before-changing-anything)
- [4 · See what installing would do](#4--see-what-installing-would-do)
- [5 · Install](#5--install)
- [6 · Read your highlights](#6--read-your-highlights)
- [7 · Keep them, and ask what is new](#7--keep-them-and-ask-what-is-new)
- [8 · Export to Markdown](#8--export-to-markdown)
- [9 · Connect Zotero (optional)](#9--connect-zotero-optional)
- [10 · The terminal interface](#10--the-terminal-interface)
- [11 · Remove it completely](#11--remove-it-completely)
- [What is not here yet](#what-is-not-here-yet)
- [If something goes wrong](#if-something-goes-wrong)

---

## Before you start

**What you need**

| | |
|---|---|
| A reMarkable 2 | any firmware; unvalidated ones run read-only, which is everything below |
| **Rust** | `rust-toolchain.toml` pins the version and rustup installs it for you |
| **Docker** | only for building the device binary — nothing else uses it |
| SSH access to the device | Settings → Help → About → Copyrights and licenses |

There is no Node, no JavaScript, and no desktop application to install.

**What this will and will not do to your reMarkable**

Everything Marginalia places lives in **one directory**, `/home/root/.marginalia`.
It never modifies `xochitl`, the kernel, a system partition or a startup entry;
never touches a document it did not put there; and never deletes anything to
free space. [§11](#11--remove-it-completely) removes it and verifies it is gone.

**A word about the SSH password.** It is on the device, under Settings → Help →
About. Never put it in a file in this repository. If you have ever shared it,
toggle developer access off and on — the password is regenerated.

---

## 1 · Get the code

```bash
git clone https://github.com/cyuss/marginalia.git && cd marginalia
```

Confirm the core works before it goes anywhere near a device. This needs only
Rust and takes about a minute:

```bash
make test
```

```
test result: ok. 424 passed; 0 failed
```

The safety suite is worth running on its own, because a failure there is never
acceptable:

```bash
make test-safety
```

---

## 2 · Find your reMarkable

Over **USB**, the address is always `10.11.99.1` and you need nothing else.

Over **Wi-Fi**, read the address from Settings → Help → About. Set it once:

```bash
export RM_HOST=192.168.1.190     # your address here
```

Every command below honours `RM_HOST`, and defaults to the USB address without
it.

---

## 3 · Check, before changing anything

```bash
make device-doctor
```

```
Marginalia — device check
Nothing is written. Every command below only reads.

Your machine
  ✓ cargo 1.90.0
  ✓ cross (builds for the reMarkable's ARM processor)

Your reMarkable
  ✓ reachable at 192.168.1.190
  · firmware 3.28.0.166
  · 1090 MB free in /home
  · Marginalia is not installed

What Marginalia will and will not touch
  ✓ writes only: /home/root/.marginalia
  never: /usr /etc /lib /bin /boot /opt — the device's own software
  never: xochitl, the kernel, the bootloader, firmware updates
  never: your notebooks, or any document Marginalia did not create
```

You will be asked for the device's password. That is `ssh` asking, not
Marginalia — nothing stores it.

---

## 4 · See what installing would do

Every step, performed on nothing:

```bash
make device-install-dry
```

```
1 · Finding your reMarkable
  ✓ connected at 192.168.1.190
  · firmware 3.28.0.166
  ! This firmware has not been validated with Marginalia.
  · The agent will run read-only until it has been.

3 · Checking there is room
  · 1090 MB free
  ✓ enough room, with the reserve intact

4 · Copying into /home/root/.marginalia
  · would create /home/root/.marginalia/bin
  · would copy the agent to /home/root/.marginalia/bin/marginalia
  · would write /home/root/.marginalia/install-manifest.tsv
  · would run: /home/root/.marginalia/bin/marginalia init

Dry run complete. Nothing was changed.
```

The firmware warning is expected and is not a failure. No firmware has been
validated for **writes** yet, and nothing below needs one.

---

## 5 · Install

```bash
make device-install
```

The first run builds the ARM binary in a container and takes a few minutes.
Later runs reuse the cache and take seconds.

```
4 · Copying into /home/root/.marginalia
  ✓ created /home/root/.marginalia
  ✓ copied the agent
  ✓ verified by checksum

5 · Recording what was installed
  ✓ manifest written

6 · Setting up the agent
ready
  home     /home/root/.marginalia
  database /home/root/.marginalia/marginalia.sqlite (schema v3)
  journal  delete
  ✓ database created
```

**The checksum line matters.** What arrived is compared against what was sent,
and a mismatch removes the file rather than leaving a half-copied binary that
runs.

Ask the agent how it is:

```bash
make device-status
```

```
  home        /home/root/.marginalia
  initialised yes
  schema      v3
  zotero      not connected

Permitted right now
   no send documents to this device
   no write annotations into PDFs
   no two-way tag sync

Never, under any setting
  no  modify the reMarkable's own software
  no  touch a document Marginalia did not put here
  no  delete anything to free space
```

Three "no"s under *Permitted right now* is correct on an unvalidated firmware.
It is the safety model refusing every write, and you can watch it do so.

---

## 6 · Read your highlights

**First, highlight something.** On the reMarkable, open a PDF or EPUB, select
text with the stylus, and choose the highlighter. Marginalia reads what the
device already stored — it adds nothing to how you read.

```bash
ssh root@$RM_HOST '/home/root/.marginalia/bin/marginalia highlights'
```

```
26 document(s), 2624 highlight(s)

  Albano et al. - 2015 - Relational DBMS Internals       200  pdf
  An introduction to Science                             201  pdf
  Cultures and Organizations Software for the Mind, T…   546  pdf
  Downey - 2012 - Think Complexity                        77  pdf
  God and Time                                            11  pdf
  Inverting the Pyramid: The History of Football Tact…    68  epub
  …

  marginalia highlights <part of a title>   to read them
  marginalia highlights --export            to write them to a file
```

The passages themselves, by any part of the title:

```bash
ssh root@$RM_HOST '/home/root/.marginalia/bin/marginalia highlights "Think Complexity"'
```

```
Downey - 2012 - Think Complexity
───────────────────────────────

  a graph is a set of nodes and a set of edges
      — page 12
```

As JSON, for scripting:

```bash
ssh root@$RM_HOST '/home/root/.marginalia/bin/marginalia highlights --document <uuid>'
```

**This only ever reads.** No document is modified, moved or re-saved. If a file
cannot be read it is *listed with the reason*, never skipped silently — that
choice is why a colour-field bug was caught instead of quietly losing ten
documents.

---

## 7 · Keep them, and ask what is new

Reading is stateless. Saving is what makes *history* possible:

```bash
ssh root@$RM_HOST '/home/root/.marginalia/bin/marginalia highlights --save'
```

```
2624 highlight(s) across 26 document(s)
  2624 new since the last run
  2624 kept in total
```

Run it again and nothing duplicates, because a highlight's identity comes from
its own text and position rather than a counter:

```
2624 highlight(s) across 26 document(s)
  nothing new since the last run
  2624 kept in total
```

That takes **about a second** for 2,624 highlights.

Now go and highlight something new on the device, run `--save` again, and ask:

```bash
ssh root@$RM_HOST '/home/root/.marginalia/bin/marginalia highlights --new'
```

```
3 new highlight(s) since 2026-08-12T23:44:28Z

Peter Godfrey-Smith - Theory and Reality
  science is not a single method but a family of them
      — page 41
```

**Nothing is ever deleted.** If you remove a highlight on the device, the record
is *marked* as gone, not destroyed — reading you did is not the device's to
retract.

---

## 8 · Export to Markdown

```bash
ssh root@$RM_HOST '/home/root/.marginalia/bin/marginalia highlights --export'
scp -r root@$RM_HOST:/home/root/.marginalia/highlights/ .
```

One file per document, each passage a blockquote with its page:

```markdown
# Downey - 2012 - Think Complexity

> a graph is a set of nodes and a set of edges
>
> — page 12

---

Extracted by Marginalia (extraction v1, format verified against firmware 3.28.0.166).
The reMarkable's own files remain the source of truth.
```

Paste it into Obsidian, a Zotero note, or anything else. The files are yours and
readable without Marginalia.

The export is written **inside Marginalia's own directory** on the device, never
into your library.

---

## 9 · Connect Zotero (optional)

Zotero is one place documents can come from. A plain folder works too, and
nothing else changes if you skip this.

Create a key at [zotero.org/settings/keys](https://www.zotero.org/settings/keys),
then:

```bash
ssh root@$RM_HOST '/home/root/.marginalia/bin/marginalia zotero connect <your-key>'
ssh root@$RM_HOST '/home/root/.marginalia/bin/marginalia sync'
```

Sync brings titles, authors, collections and tags. **It never transfers a PDF** —
moving a document is always a separate, explicit request.

To disconnect:

```bash
ssh root@$RM_HOST '/home/root/.marginalia/bin/marginalia zotero disconnect'
```

That forgets the key on the device. It does **not** revoke it at Zotero — do
that yourself, on the same settings page.

---

## 10 · The terminal interface

Everything above, without memorising commands:

```bash
make tui
```

```
Marginalia   device 192.168.1.190   (h to change)

YOUR REMARKABLE
    Check the device
    Show what installing would do
    Install or update
    Ask the agent how it is
YOUR READING
    List what you have highlighted
    Write the highlights to files
    Copy those files to this computer
…
REMOVING IT
  ! Remove Marginalia from the device

Reads only. Reports firmware, free space, and what Marginalia will never touch.
Runs in this terminal, so you can answer any prompt yourself.
$ ./tools/device/doctor.sh

↑↓ move   ⏎ run   h device address   q quit
```

It shows the command each entry will run **before** you press enter, so you never
have to trust a label. Anything touching the device hands the terminal back to
you for the password, and removal asks twice — once here, once in the script.
Nothing answers a safety prompt on your behalf.

---

## 11 · Remove it completely

See what would go, first:

```bash
make device-reset-dry
```

Then:

```bash
make device-reset
```

```
2 · What will be removed
  · /home/root/.marginalia
  · 3 file(s), 4.2M

      /home/root/.marginalia/install-manifest.tsv
      /home/root/.marginalia/bin/marginalia
      /home/root/.marginalia/marginalia.sqlite

3 · Checking nothing outside that directory is involved
  ✓ every installed file is inside /home/root/.marginalia
  ✓ no system services, startup entries or modified system files

Type remove to continue: remove

5 · Verifying
  ✓ the directory is gone
  ✓ your home directory is intact
  ✓ nothing named Marginalia remains

Done. Your reMarkable is back to stock.
```

There is nothing to restore, because nothing was replaced. Verified on a device
with 3,814 documents and 27 highlight folders: identical before and after, with
`xochitl` running throughout.

---

## What is not here yet

Honesty is cheaper than disappointment.

| | |
|---|---|
| **A Zotero folder on the device**, browsable, metadata-only, with tick-to-download | designed, authorised, **not built** |
| Review documents generated into your library | needs the same library-write validation |
| Sending a document to the device on request | Phase 3 |
| Handwritten margin notes | needs a `.rm` v6 stroke parser |

**There is no Marginalia interface on the reMarkable screen, and there will not
be.** No split view, no sidebar, no entry beside Google Drive in Integrations —
that sidebar is reMarkable's own cloud feature and has no third-party API.
Drawing on the screen *is* possible by patching the `xochitl` binary, which is
what `ddvk/remarkable-hacks` does; it is refused here rather than impossible,
because leaving your device's software alone is the reason this is safe to
install. The full reasoning is in
[ADR-002](adr/ADR-002-remarkable-ui-and-runtime.md).

---

## If something goes wrong

**`Could not read the reMarkable's document store`** — `highlights` reads the
device's own library, so it runs *on* the reMarkable over SSH, not on your
computer.

**`This firmware has not been validated`** — expected. The agent runs read-only,
which is everything in this guide.

**`N document(s) could not be read`** — listed on purpose, with a reason each.
Nothing was changed; please open an issue with the message.

**The build fails with SIGBUS inside rustc** — clear the container's cache and
try again:

```bash
docker volume rm marginalia-armv7-target
```

**SSH stops responding after several commands** — the device accepts few
concurrent sessions. Wait a moment; the device itself is fine.

---

## Where the guarantees are written down

- [SAFETY_MODEL.md](safety/SAFETY_MODEL.md) — what protects your device
- [DEVICE_WRITE_POLICY.md](safety/DEVICE_WRITE_POLICY.md) — exactly what may be written
- [HARDWARE_VALIDATION.md](remarkable/HARDWARE_VALIDATION.md) — what a real device did, measured
- [ROADMAP.md](../ROADMAP.md) — what is kept, what is excluded, and why
