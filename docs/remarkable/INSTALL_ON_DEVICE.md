# Installing Marginalia on your reMarkable

Connect the device, run three commands, and everything Marginalia does lives on
the reMarkable itself. Your computer is needed to install it, and never again
after that.

> ### ⚠ Nothing here has been run against real hardware
>
> These scripts are written and reviewed, and their guard rails are covered by
> tests — but **no reMarkable has been touched**. Whether the agent runs on your
> firmware is [U11 and U13](../development/OPEN_QUESTIONS.md), still open.
>
> They cannot damage your device's software: everything they write goes into one
> directory they create, and [`reset.sh`](#removing-it) removes it. But they may
> simply not work yet, and you should read `--dry-run` output before believing
> any of it.

---

## What gets installed

One directory:

```
/home/root/.marginalia/
├── bin/marginalia            the agent
├── marginalia.sqlite         its database
├── zotero_api_key.secret     your key, mode 0600
└── install-manifest.tsv      every file above, with checksums
```

That is the whole footprint. No system files, no startup entries, no changes to
`xochitl`, the kernel, the bootloader or firmware updates. Removing that
directory removes Marginalia completely — which is what
[`reset.sh`](#removing-it) does, and then proves.

## Before you start

**On the reMarkable**, enable developer access: *Settings → Help → About →
Copyrights and licenses*. It shows an SSH password. You control this setting and
can turn it off again afterwards.

**On your machine**, you need Rust and a cross-compiler. The reMarkable uses a
32-bit ARM processor, so the agent has to be built for it:

```bash
cargo install cross
```

That needs Docker. There is an alternative if you already have
`arm-linux-gnueabihf-gcc`. See
[ADR-003](../adr/ADR-003-cross-compilation.md).

## Install

Connect the reMarkable by USB and switch it on.

**1. Check everything is ready.** Reads only; changes nothing:

```bash
./tools/device/doctor.sh
```

It reports what it found, how much space you have, and — last — the list of
things Marginalia will never touch.

**2. See what installing would do.** Still changes nothing:

```bash
./tools/device/install.sh --dry-run
```

**3. Install:**

```bash
./tools/device/install.sh
```

It builds the agent, checks there is room, copies one file, **verifies it
arrived intact by checksum**, writes the manifest, and creates the database. If
the copy does not verify, it removes what it sent and stops.

## Connect your Zotero library

Create a key at [zotero.org/settings/keys](https://www.zotero.org/settings/keys)
— *Create new private key*, allow library access, and add write access only if
you want to export annotations back.

You do not need a library ID. Marginalia asks Zotero for it.

```bash
ssh root@10.11.99.1 '/home/root/.marginalia/bin/marginalia status'
```

> **Not yet wired.** The setup command that accepts the key on the device is
> the next piece of work — the logic is written and tested in
> `packages/zotero`, but the agent does not yet expose it. Today `status` will
> report *zotero: not connected*.

## Where the features appear

Marginalia never draws to your reMarkable's screen. It has no interface of its
own and never takes over the one you have.

Instead, everything appears as **documents in your library**, which the native
reader already renders beautifully:

| Document | What it is |
|---|---|
| `Marginalia / Library` | your Zotero library, with a tick box beside each paper |
| `Marginalia / Annotation Inbox` | every highlight and note, across every document |
| `Marginalia / Activity` | what Marginalia did, and what it refused to do |

**To ask for a paper, tick its box with your stylus.** Marginalia reads the page
the next time it syncs and downloads that one. Not the ones above it, not the
whole collection — the one you ticked.

That is the whole interaction model, and the reasoning is in
[ADR-006](../adr/ADR-006-on-device-interaction.md). It means Marginalia needs
only to *read* your annotations, which is why it can leave the rest of your
device entirely alone.

> **Not yet built.** Generating those documents is Phase 5 — it needs the PDF
> layer, which is blocked on [U15](../development/OPEN_QUESTIONS.md). The logic
> that turns a stylus mark into a request is implemented and tested today, in
> `marginalia_core::request_form`.

## Checking on it

```bash
ssh root@10.11.99.1 '/home/root/.marginalia/bin/marginalia status'
```

```
Marginalia 0.1.0

  home        /home/root/.marginalia
  initialised yes
  schema      v1
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

Everything reads *no* because nothing has been validated on real hardware yet.
That is the correct state, and it is enforced rather than displayed: the
compatibility matrix ships with every capability `UNKNOWN`, and unknown never
permits a write.

If something looks wrong:

```bash
ssh root@10.11.99.1 '/home/root/.marginalia/bin/marginalia doctor'
```

## Removing it

```bash
./tools/device/reset.sh --dry-run    # show exactly what would go
./tools/device/reset.sh              # remove it
```

The reset script lists every file first, checks that all of them are inside
Marginalia's own directory, confirms there are no system services or startup
entries anywhere, asks you to type `remove`, deletes the directory, and then
**verifies it is gone and that your home directory is intact**.

There is nothing to restore afterwards. Marginalia never modified your
reMarkable's software, so returning to stock is removing one directory — and the
script exists mostly to prove that claim rather than ask you to trust it.

Documents you asked Marginalia to download are **yours**. They are ordinary PDFs
in your library and reset leaves them alone. Delete them from the reMarkable
itself if you want them gone.

To also revoke the Zotero key you gave it, go back to
[zotero.org/settings/keys](https://www.zotero.org/settings/keys).

## Does it run on its own?

Not yet, and not by accident.

Making the agent start automatically means a startup entry, and
[the device write policy](../safety/DEVICE_WRITE_POLICY.md) forbids creating
one — that is persistent system configuration, which is exactly the category
Marginalia promises to stay out of.

For now you run it when you want it:

```bash
ssh root@10.11.99.1 '/home/root/.marginalia/bin/marginalia status'
```

Automatic running needs a decision recorded in an ADR, weighing the least
invasive option against that promise. It is not something an installer should
quietly do to your device.

## If something goes wrong

| What you see | What it means |
|---|---|
| `cannot reach your reMarkable` | Not connected, off, or developer access not enabled. Check `ssh root@10.11.99.1`. |
| `no ARM cross-compiler found` | Run `cargo install cross` (needs Docker). |
| `the copy did not arrive intact` | The transfer was corrupted. The partial file was removed; nothing else changed. Run install again. |
| `less than 50 MB free` | Marginalia will not fill your device. Free some space. |
| `MARGINALIA_HOME is inside /usr` | A guard caught a misconfiguration. Unset `MARGINALIA_HOME` and try again. |

Every one of these stops before changing anything. If a script fails partway,
`./tools/device/reset.sh` cleans up whatever it managed to place.
