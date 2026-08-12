# Installing Marginalia

A complete, step-by-step guide to getting Marginalia running on your machine.

> **Before you start.** Marginalia is at **Phase 0**. What builds and runs today
> is the foundation: the domain model, the safety layer, the database, the
> device simulator, and the application shell. There is **no Zotero connection
> and no reMarkable connection yet** — those arrive in Phases 1 and 2. See
> [ROADMAP.md](../ROADMAP.md).
>
> Nothing in this build can modify a reMarkable. There is no device transport
> code at all.

---

## Table of contents

1. [What you need](#1-what-you-need)
2. [Install the prerequisites](#2-install-the-prerequisites)
3. [Get the code](#3-get-the-code)
4. [Verify the core](#4-verify-the-core)
5. [Run the terminal interface](#5-run-the-terminal-interface)
5b. [Connect your Zotero library](#5b-connect-your-zotero-library)
6. [Build the agent for your reMarkable](#6-build-the-agent-for-your-remarkable)
7. [Everyday commands](#7-everyday-commands)
8. [Project layout](#8-project-layout)
9. [Troubleshooting](#9-troubleshooting)
10. [Uninstalling](#10-uninstalling)

---

## 1. What you need

| Tool | Minimum | Why |
|---|---|---|
| **Rust** | see `rust-toolchain.toml` | rustup installs the pinned version for you on first build. |
| **Git** | any recent | To clone the repository. |
| **A C compiler** | platform default | SQLite is compiled from source (`rusqlite` bundled). |

Disk space: roughly **3 GB** once Rust dependencies are compiled.
First build: **5–15 minutes**. Later builds are seconds.

You do **not** need a reMarkable. You do **not** need Zotero. Development runs
against a simulator and fixtures.

---

## 2. Install the prerequisites

### macOS

```bash
xcode-select --install
```

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```


Nothing else is needed on macOS beyond the Xcode command line
tools.

### Linux (Debian / Ubuntu)

```bash
sudo apt update && sudo apt install -y build-essential curl file libssl-dev pkg-config
```

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

```bash
```


### Windows

1. Install [Rust](https://rustup.rs) (the installer will offer to install the
   Visual Studio C++ Build Tools — accept).


### Check your versions

```bash
rustc --version
```

`rust-toolchain.toml` pins the version this project is tested against, and
rustup installs it for you on the first build — so a mismatch here is not a
problem. If something goes wrong anyway, see
[Troubleshooting](#9-troubleshooting).

---

## 3. Get the code

```bash
git clone https://github.com/cyuss/marginalia.git && cd marginalia
```

---

## 4. Verify the core

This is the fastest way to confirm your setup, and it is where the interesting
part of the project lives. It needs only Rust.

```bash
cargo test --workspace
```

Then run the safety suite on its own:

```bash
cargo test -p marginalia-safety-suite -- --nocapture
```

These tests assert the guarantees the project is built around — that unknown
firmware denies every write, that a metadata sync transfers zero PDFs, that a
document Marginalia did not put on your device is never modified, and that a
prohibited operation is refused no matter how the app is configured.

**If the safety suite fails, stop and open an issue.** Do not use the build.

---

## 5. Run the terminal interface

```bash
make tui
```

This is where you install Marginalia on a reMarkable, check it, configure a
library source, and remove it again — without memorising commands.

It is a front door, not a second implementation: every entry runs one of the
scripts in `tools/device/` or the agent over SSH, and the command it will run is
shown on screen before you press enter. Anything touching the device hands the
terminal back to you, so you answer the password prompt and type your own
confirmations. Nothing here answers a safety question on your behalf.

Press `h` to set your device's address. `q` quits.

> There is no graphical application, and no Marginalia interface on the
> reMarkable itself. That is a decision, not a gap —
> [ADR-002](adr/ADR-002-remarkable-ui-and-runtime.md) explains what was
> considered and why it was refused.

---

## 5b. Connect your Zotero library

Marginalia reads your Zotero library through the Zotero Web API. **You only
need one thing: an API key.**

### Where do I find my library ID?

You don't. Marginalia asks Zotero.

Your library ID is your numeric Zotero user ID, and Zotero will report it when
asked about a key — so the setup screen has no field for it. If you are curious,
or you need it for something else, it is shown at
[zotero.org/settings/keys](https://www.zotero.org/settings/keys) as *"Your
userID for use in API calls"*. It is a number, not the URL of your profile page.

### Create the key

1. Go to **[zotero.org/settings/keys](https://www.zotero.org/settings/keys)**.
2. Click **Create new private key**. Name it something you will recognise
   (`Marginalia`).
3. Grant the **minimum** it needs:
   - *Allow library access* — required; this is the metadata sync.
   - *Allow write access* — only if you want to export annotations back to
     Zotero. You can add it later by editing the key.
4. Copy it. **Zotero shows the key once.**

### Add it during setup

In the app: **Zotero → Connect**, paste the key, press Connect.

Marginalia asks Zotero what the key is, and:

| What it finds | What happens |
|---|---|
| One library | Connects straight away. Nothing else to answer. |
| Several (personal + groups) | Shows the list so you choose. **Nothing is stored until you do** — otherwise you would end up configured against a library you never picked. |
| A key with no library access | Says so, and points at the permission checkbox you probably missed. |
| A key Zotero rejects | Says so. **The key never reaches your disk.** If you already had a working key, it is untouched. |

Verification always happens **before** storage. A key that does not work is
never saved, so setup cannot appear to succeed while nothing works.

Where the key is stored once accepted:

| Platform | Location |
|---|---|
| reMarkable | one file per secret in Marginalia's own data directory, mode `0600` |
| Linux / macOS | the same, until the OS-keychain integration lands |
| Windows | the app's data directory, protected by its ACL |

To disconnect: **Zotero → Disconnect**. That deletes Marginalia's copy. It does
**not** revoke the key at Zotero — only you can do that, from the same settings
page where you created it.

### Running the live tests against your library

The test suite is offline and deterministic by default. To exercise the real
API, supply the credentials through your environment:

```bash
export MARGINALIA_ZOTERO_API_KEY=your-key-here
cargo test -p marginalia-zotero --features http -- --ignored --nocapture
```

That is enough — the key-only tests discover the library themselves. One test
prints exactly what your key can reach:

```
user ID   : 1234567
username  : Some("youcef")
personal  : Some(LibraryAccess { read: true, write: false, notes: true, files: true })
groups    : []
all groups: false
```

`MARGINALIA_ZOTERO_LIBRARY_ID` is optional, and only exercises the older
explicit-library path. Without any variables the live tests skip and say why.

### Keeping the key safe

- **Never commit it.** Not in a test file, a fixture, a screenshot, or a config
  file that is tracked. `.gitignore` covers `.env`, `.env.local` and `*.secret`,
  but the reliable defence is not putting it there in the first place.
- **A key that has been pasted into a chat, an issue, or a pull request is
  compromised.** Revoke it and create a new one — revocation is instant and
  free.
- Grant the minimum permissions the features you use actually need.
- Marginalia never logs the key. It is held in a type that renders as
  `<redacted>` in every format, including debug output, so a careless log line
  cannot leak it.

## 6. Build the agent for your reMarkable

```bash
make build-device-docker    # needs only Docker
make verify-device-binary   # runs that ARM binary under emulation
```

The result is a single static-ish ARM binary of about 4 MB. `make device-install`
(or the terminal interface) is what puts it on a device.

---

## 7. Everyday commands

```bash
make tui            # the terminal interface
make test           # the full test suite
make test-safety    # the safety suite alone
make test-arch      # dependency-direction and forbidden-import rules
make lint           # clippy, warnings denied
make fmt            # format
make check          # everything CI runs, in order
make device         # what you can do to a connected reMarkable
```

Run `make check` before opening a pull request. Every recipe exists in both
`make` and `just`, under the same name.

---

## 8. Project layout

```
marginalia/
├── apps/
│   ├── remarkable/          the on-device agent — this is the product
│   └── tui/                 the terminal interface, on your computer
│
├── packages/
│   ├── core/                domain model, state machines — depends on nothing
│   ├── safety/              SafetyManager, WriteGrant, flags, snapshots
│   ├── database/            SQLite, migrations, repositories
│   ├── remarkable/          device port + firmware compatibility matrix
│   ├── annotations/         reads the highlights the device already stored
│   ├── zotero/              a LibraryProvider
│   ├── library-folder/      a LibraryProvider that needs no network
│   └── observability/       structured logging, SAFETY audit channel
│
├── tests/
│   ├── remarkable-simulator/  the simulated device
│   └── safety/                the mandatory safety suite
│
└── docs/                    architecture, safety model, open questions
```

One workspace, one toolchain, no JavaScript. There was a second workspace for a
Tauri desktop shell; it was removed in August 2026, having never built. You can
work on the entire domain and safety layer with `cargo test` alone, and you
never need a device.

---

## 9. Troubleshooting

### `cargo build` fails with an edition or feature error

Your Rust is older than 1.77.

```bash
rustup update stable
```

`rust-toolchain.toml` pins the version, and rustup installs it automatically —
`cargo test --workspace` from the repository root should simply work.

### `linker cc not found` / `error: linker not found`

Install a C toolchain: `xcode-select --install` on macOS, `build-essential` on
Debian/Ubuntu, the Visual Studio C++ Build Tools on Windows. SQLite is compiled
from source.

### The first build seems stuck

It is compiling several hundred crates, plus SQLite from C source. Watch
progress:

```bash
cargo build --workspace --verbose
```

### Where is my data?

Marginalia stores a local SQLite database and nothing else. There is no
account, no server, no telemetry. On a device it lives in
`/home/root/.marginalia`, and `marginalia status` prints the path.

---

## 10. Uninstalling

Delete the repository directory. If you installed a built app, drag it to the
trash (macOS), uninstall it from Settings (Windows), or remove the package
(Linux).

Marginalia writes only to its own local database and does not install services,
daemons, or startup entries.

**And on your reMarkable:** there is nothing to uninstall. Marginalia never
installs anything on the device, and this build cannot write to one at all.
