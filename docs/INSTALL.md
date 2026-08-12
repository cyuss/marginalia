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
4. [Verify the core (no Node needed)](#4-verify-the-core-no-node-needed)
5. [Run the desktop app](#5-run-the-desktop-app)
5b. [Connect your Zotero library](#5b-connect-your-zotero-library)
6. [Build a distributable app](#6-build-a-distributable-app)
7. [Everyday commands](#7-everyday-commands)
8. [Project layout](#8-project-layout)
9. [Troubleshooting](#9-troubleshooting)
10. [Uninstalling](#10-uninstalling)

---

## 1. What you need

| Tool | Minimum | Why |
|---|---|---|
| **Rust** | 1.77+ | 1.77 is required by Tauri 2. The core crates alone build on 1.68+. |
| **Node.js** | 20.10+ | Required by Vite 5 and the Tauri CLI. |
| **pnpm** | 9+ | The workspace uses pnpm workspaces. |
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

Then install Node 20+ and pnpm. If you use [nvm](https://github.com/nvm-sh/nvm):

```bash
nvm install 20 && nvm use 20 && corepack enable && corepack prepare pnpm@9 --activate
```

Tauri needs no extra system packages on macOS beyond the Xcode command line
tools.

### Linux (Debian / Ubuntu)

```bash
sudo apt update && sudo apt install -y build-essential curl file libssl-dev libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev pkg-config
```

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

```bash
nvm install 20 && nvm use 20 && corepack enable && corepack prepare pnpm@9 --activate
```

On Fedora, the WebKit package is `webkit2gtk4.1-devel`; on Arch, `webkit2gtk-4.1`.

### Windows

1. Install [Rust](https://rustup.rs) (the installer will offer to install the
   Visual Studio C++ Build Tools — accept).
2. Install [Node.js 20 LTS](https://nodejs.org).
3. Enable pnpm:

```bash
corepack enable && corepack prepare pnpm@9 --activate
```

WebView2 ships with Windows 10/11. If it is missing, install the
[Evergreen Runtime](https://developer.microsoft.com/microsoft-edge/webview2/).

### Check your versions

```bash
rustc --version && node --version && pnpm --version
```

You want Rust ≥ 1.77, Node ≥ 20.10, pnpm ≥ 9. If any is older, see
[Troubleshooting](#9-troubleshooting).

---

## 3. Get the code

```bash
git clone https://github.com/USER/marginalia.git && cd marginalia
```

Install the JavaScript dependencies:

```bash
pnpm install
```

---

## 4. Verify the core (no Node needed)

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

## 5. Run the desktop app

```bash
pnpm dev
```

This starts Vite and launches the Tauri window. The first run compiles the Rust
side and takes several minutes; later runs are fast, and the UI hot-reloads.

To work on the interface alone, in a normal browser at
`http://localhost:1420`:

```bash
pnpm dev:web
```

What you will see: the application shell — Library, Annotation Inbox, Zotero,
Device, Activity and Settings — with honest empty states. There is no mock
data. The Device screen states plainly what Marginalia may and may not do.

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

## 6. Build a distributable app

```bash
pnpm build
```

Output lands in `apps/desktop/src-tauri/target/release/bundle/`:

- macOS — `.app` and `.dmg`
- Windows — `.msi` and `.exe`
- Linux — `.deb`, `.rpm` and `.AppImage`

Builds are unsigned. Signing and notarisation are a release concern, not a
development one.

---

## 7. Everyday commands

```bash
pnpm dev            # run the desktop app
pnpm dev:web        # run the UI only, in a browser
pnpm build          # build a distributable app
pnpm test           # the full Rust test suite
pnpm test:safety    # the safety suite alone
pnpm typecheck      # TypeScript, strict
pnpm lint           # ESLint
pnpm check          # everything above, in order
cargo fmt --all     # format Rust
```

Run `pnpm check` before opening a pull request.

---

## 8. Project layout

```
marginalia/
├── apps/desktop/            Tauri 2 shell — React UI + Rust binary
│   └── src-tauri/           its own Cargo workspace (needs newer Rust)
│
├── packages/
│   ├── core/                domain model, state machines — depends on nothing
│   ├── safety/              SafetyManager, WriteGrant, flags, snapshots
│   ├── database/            SQLite, migrations, repositories
│   ├── remarkable/          device port + firmware compatibility matrix
│   └── observability/       structured logging, SAFETY audit channel
│
├── tests/
│   ├── remarkable-simulator/  the simulated device
│   └── safety/                the mandatory safety suite
│
└── docs/                    architecture, safety model, open questions
```

Two Cargo workspaces, deliberately. The root workspace holds the core crates
and builds on a modest toolchain; `apps/desktop/src-tauri` is separate because
Tauri 2 needs a much newer Rust. You can work on the entire domain and safety
layer with `cargo test` alone.

---

## 9. Troubleshooting

### `pnpm` fails with `URL.canParse is not a function`

Your Node is older than 20. Corepack's pnpm shim needs Node 20+.

```bash
nvm install 20 && nvm use 20 && corepack prepare pnpm@9 --activate
```

### `cargo build` fails in `apps/desktop/src-tauri` with an edition or feature error

Your Rust is older than 1.77.

```bash
rustup update stable
```

The core crates still build and test on 1.68 — run `cargo test --workspace`
from the repository root, which does not include the Tauri app.

### `linker cc not found` / `error: linker not found`

Install a C toolchain: `xcode-select --install` on macOS, `build-essential` on
Debian/Ubuntu, the Visual Studio C++ Build Tools on Windows. SQLite is compiled
from source.

### Tauri fails on Linux with a missing `webkit2gtk` package

Install the development packages listed in
[§2](#2-install-the-prerequisites). The package name varies by distribution;
on newer distributions it is the `4.1` variant.

### The first build seems stuck

It is compiling several hundred crates, plus SQLite from C source. Watch
progress:

```bash
cargo build --workspace --verbose
```

### Tests pass but the app window is blank

Check that Vite is serving on port 1420, and look at the terminal running
`pnpm dev` for a TypeScript error. The Tauri window loads
`http://localhost:1420` in development.

### Where is my data?

Marginalia stores a local SQLite database and nothing else. There is no
account, no server, no telemetry. Once Phase 1 lands, the database path is
shown in Settings.

---

## 10. Uninstalling

Delete the repository directory. If you installed a built app, drag it to the
trash (macOS), uninstall it from Settings (Windows), or remove the package
(Linux).

Marginalia writes only to its own local database and does not install services,
daemons, or startup entries.

**And on your reMarkable:** there is nothing to uninstall. Marginalia never
installs anything on the device, and this build cannot write to one at all.
