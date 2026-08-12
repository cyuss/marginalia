//! # Architecture tests
//!
//! These enforce the dependency direction that makes a standalone reMarkable 2
//! runtime possible at all. They are the mechanism behind the roadmap rule:
//!
//! > `core` is deterministic, portable, and unaware of Tauri, React, RM2,
//! > `xochitl`, SSH, OS keychains, HTTP clients, SQLite implementations, and UI
//! > frameworks.
//!
//! A dependency-direction rule that lives only in a document decays. These
//! tests read the actual manifests and sources, so a forbidden edge fails CI
//! rather than being noticed in review three months later.
//!
//! They are deliberately written against the *current* layout. When Phase 0.5
//! renames `packages/` to `crates/`, this file is the thing that must be
//! updated first, and updating it is a visible, reviewable decision.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

/// Crates that must be able to compile for the reMarkable 2 without dragging in
/// anything desktop-shaped.
const PORTABLE_CRATES: &[&str] = &[
    "marginalia-core",
    "marginalia-safety",
    "marginalia-observability",
    "marginalia-database",
    "marginalia-remarkable",
    "marginalia-platform",
    "marginalia-zotero",
    "marginalia-sync",
    "marginalia-library-folder",
    "marginalia-annotations",
];

/// Adapters: the layer whose *job* is to know about a host.
///
/// They must still cross-compile and must still avoid desktop-only
/// dependencies, but `#[cfg(unix)]` is legitimate here — that is the whole
/// point of an adapter. Everywhere else it is a platform assumption leaking
/// into logic that should not have one.
const ADAPTER_CRATES: &[&str] = &[
    "marginalia-platform",
    "marginalia-zotero",
    "marginalia-library-folder",
];

/// Crates where a platform branch is a defect: domain and application logic.
fn is_platform_agnostic(name: &str) -> bool {
    PORTABLE_CRATES.contains(&name) && !ADAPTER_CRATES.contains(&name)
}

/// External crates that must never appear in a portable crate's dependency
/// list. Each is a proxy for a whole category of non-portability.
const FORBIDDEN_EXTERNAL_DEPS: &[(&str, &str)] = &[
    ("tauri", "desktop shell"),
    ("tauri-build", "desktop shell"),
    ("wry", "desktop webview"),
    ("tao", "desktop windowing"),
    // The terminal interface is a host program and may use these. A crate that
    // has to run on the reMarkable may not: the agent is headless, there is no
    // terminal attached to it, and drawing anywhere is what ADR-002 rules out.
    ("ratatui", "terminal interface — host only"),
    ("crossterm", "terminal control — host only"),
    ("napi", "Node binding"),
    ("neon", "Node binding"),
    (
        "keyring",
        "desktop OS secret store — the RM2 needs its own adapter",
    ),
    (
        "ssh2",
        "host-side device transport, not something the device itself uses",
    ),
    ("russh", "host-side device transport"),
    ("winapi", "platform-specific"),
    ("windows", "platform-specific"),
    ("cocoa", "platform-specific"),
    ("objc", "platform-specific"),
];

/// Source-level markers of the same problem, for cases where a dependency is
/// pulled in transitively or a target-specific assumption is written by hand.
const FORBIDDEN_SOURCE_MARKERS: &[(&str, &str)] = &[
    ("use tauri", "Tauri import"),
    ("tauri::", "Tauri path"),
    ("#[tauri::command]", "Tauri command"),
    ("keyring::", "desktop keychain"),
    (
        "xochitl",
        "the native reMarkable application must never be referenced in code",
    ),
];

/// The only workspace-internal edges that are allowed.
///
/// Read this as the architecture diagram, in executable form. Adding an entry
/// is how you change the architecture; it should be a conscious act.
fn allowed_internal_deps() -> BTreeMap<&'static str, BTreeSet<&'static str>> {
    let mut m: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();

    // The domain core depends on nothing. This is the load-bearing rule.
    m.insert("marginalia-core", BTreeSet::new());

    // Logging depends on the core for one thing only: `Redacted`, which moved
    // into the domain when the credential port needed to name it. The edge
    // points inward, so it is legal — and declaring it here is the deliberate
    // act the test exists to force.
    m.insert(
        "marginalia-observability",
        ["marginalia-core"].into_iter().collect(),
    );

    m.insert(
        "marginalia-safety",
        ["marginalia-core"].into_iter().collect(),
    );
    m.insert(
        "marginalia-database",
        ["marginalia-core"].into_iter().collect(),
    );
    m.insert(
        "marginalia-remarkable",
        ["marginalia-core", "marginalia-safety"]
            .into_iter()
            .collect(),
    );
    // Host adapters implement core ports. The edge points inward, as required.
    m.insert(
        "marginalia-platform",
        ["marginalia-core"].into_iter().collect(),
    );
    m.insert(
        "marginalia-zotero",
        ["marginalia-core"].into_iter().collect(),
    );
    // A second LibraryProvider, which is what keeps the port honest.
    m.insert(
        "marginalia-library-folder",
        ["marginalia-core"].into_iter().collect(),
    );
    // Reads the reMarkable's own annotation files. Deliberately depends on the
    // core alone and NOT on marginalia-remarkable: parsing a document format is
    // not the same job as introspecting or transporting to a device, and an
    // edge between them would let format knowledge drift into the crate that
    // holds write permissions. It has no dependency on marginalia-safety
    // either, because it cannot write and so has nothing to be permitted.
    m.insert(
        "marginalia-annotations",
        ["marginalia-core"].into_iter().collect(),
    );
    // The application layer: it composes adapters and owns no rules of its own.
    // Created when a real seam appeared -- two collaborators that must not know
    // about each other, and a consumer that must not know about either.
    m.insert(
        "marginalia-sync",
        [
            "marginalia-core",
            "marginalia-database",
            "marginalia-zotero",
        ]
        .into_iter()
        .collect(),
    );

    // Test-only crates may depend on anything they are testing.
    let everything: BTreeSet<&str> = PORTABLE_CRATES
        .iter()
        .copied()
        .chain(["marginalia-simulator"])
        .collect();
    // The on-device agent composes the portable crates. It is an application,
    // not a layer, so it may depend on all of them.
    m.insert(
        "marginalia-agent",
        PORTABLE_CRATES.iter().copied().collect(),
    );

    // The terminal interface depends on nothing in the workspace, and that is
    // the point. It runs the scripts in tools/device/ and the agent over SSH,
    // so it cannot grow its own copy of the install, transfer or removal rules.
    // The day it needs to link one of these crates is the day it has started
    // duplicating logic that already exists and is already tested.
    m.insert("marginalia-tui", BTreeSet::new());

    for test_crate in [
        "marginalia-simulator",
        "marginalia-safety-suite",
        "marginalia-architecture-tests",
        "marginalia-characterization-tests",
    ] {
        m.insert(test_crate, everything.clone());
    }

    m
}

fn workspace_root() -> PathBuf {
    // CARGO_MANIFEST_DIR is <root>/tests/architecture
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("workspace root")
        .to_path_buf()
}

struct CrateManifest {
    name: String,
    dir: PathBuf,
    internal_deps: BTreeSet<String>,
    external_deps: BTreeSet<String>,
}

fn read_manifests() -> Vec<CrateManifest> {
    let root = workspace_root();
    let ws: toml::Value = toml::from_str(
        &fs::read_to_string(root.join("Cargo.toml")).expect("read workspace manifest"),
    )
    .expect("parse workspace manifest");

    let members = ws["workspace"]["members"]
        .as_array()
        .expect("members array");

    members
        .iter()
        .map(|m| {
            let dir = root.join(m.as_str().expect("member path"));
            let manifest: toml::Value =
                toml::from_str(&fs::read_to_string(dir.join("Cargo.toml")).expect("read manifest"))
                    .expect("parse manifest");

            let name = manifest["package"]["name"]
                .as_str()
                .expect("package name")
                .to_string();

            let mut internal = BTreeSet::new();
            let mut external = BTreeSet::new();
            if let Some(deps) = manifest.get("dependencies").and_then(|d| d.as_table()) {
                for key in deps.keys() {
                    if key.starts_with("marginalia-") {
                        internal.insert(key.clone());
                    } else {
                        external.insert(key.clone());
                    }
                }
            }

            CrateManifest {
                name,
                dir,
                internal_deps: internal,
                external_deps: external,
            }
        })
        .collect()
}

fn rust_sources(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let src = dir.join("src");
    let mut stack = vec![src];
    while let Some(path) = stack.pop() {
        let Ok(entries) = fs::read_dir(&path) else {
            continue;
        };
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|e| e == "rs") {
                out.push(p);
            }
        }
    }
    out
}

/// Strip comments so that a documentation mention is not mistaken for a
/// dependency. Documenting *why* we never touch `xochitl` must remain legal.
fn code_lines(source: &str) -> impl Iterator<Item = (usize, &str)> {
    source.lines().enumerate().filter_map(|(i, line)| {
        let t = line.trim_start();
        if t.starts_with("//") || t.starts_with("/*") || t.starts_with('*') {
            None
        } else {
            Some((i + 1, line))
        }
    })
}

// ── the rules ───────────────────────────────────────────────────────────────

#[test]
fn the_domain_core_depends_on_no_other_workspace_crate() {
    let core = read_manifests()
        .into_iter()
        .find(|c| c.name == "marginalia-core")
        .expect("marginalia-core exists");

    assert!(
        core.internal_deps.is_empty(),
        "marginalia-core must depend on nothing else in the workspace, found: {:?}. \
         The core is what both the reMarkable app and the desktop app share; the \
         moment it depends on something else, it stops being portable.",
        core.internal_deps
    );
}

#[test]
fn every_internal_dependency_edge_is_declared_in_the_architecture() {
    let allowed = allowed_internal_deps();

    for krate in read_manifests() {
        let Some(permitted) = allowed.get(krate.name.as_str()) else {
            panic!(
                "crate '{}' is not listed in allowed_internal_deps(). A new crate \
                 must state its place in the architecture before it can depend on \
                 anything.",
                krate.name
            );
        };

        for dep in &krate.internal_deps {
            assert!(
                permitted.contains(dep.as_str()),
                "forbidden dependency edge: {} -> {}. Allowed for {}: {:?}. \
                 Dependencies point inward; if this edge is genuinely needed, \
                 change the architecture deliberately by editing this test.",
                krate.name,
                dep,
                krate.name,
                permitted
            );
        }
    }
}

#[test]
fn portable_crates_carry_no_desktop_dependencies() {
    for krate in read_manifests() {
        if !PORTABLE_CRATES.contains(&krate.name.as_str()) {
            continue;
        }
        for (forbidden, why) in FORBIDDEN_EXTERNAL_DEPS {
            assert!(
                !krate.external_deps.contains(*forbidden),
                "{} depends on '{}' ({}). Portable crates must build for the \
                 reMarkable 2, which has no desktop toolchain.",
                krate.name,
                forbidden,
                why
            );
        }
    }
}

#[test]
fn portable_sources_contain_no_target_specific_code() {
    for krate in read_manifests() {
        if !PORTABLE_CRATES.contains(&krate.name.as_str()) {
            continue;
        }
        for file in rust_sources(&krate.dir) {
            let source = fs::read_to_string(&file).expect("read source");
            for (line_no, line) in code_lines(&source) {
                // Same escape hatch the shell guard has, and for the same
                // reason: naming a forbidden thing is not doing it. Reading the
                // directory xochitl owns is not touching xochitl -- the file
                // path has to be spelled somewhere for a read-only extractor to
                // exist at all. The marker must carry a reason, so the
                // exception is a written decision rather than a silenced test.
                if let Some(reason) = line.split("guard-allow:").nth(1) {
                    assert!(
                        !reason.trim().is_empty(),
                        "{}:{} has a bare `guard-allow:` with no reason. \
                         An exception that does not say why is not an exception, \
                         it is a hole.",
                        file.display(),
                        line_no
                    );
                    continue;
                }

                for (marker, why) in FORBIDDEN_SOURCE_MARKERS {
                    assert!(
                        !line.contains(marker),
                        "{}:{} contains '{}' ({}) outside a comment.\n  {}\n\
                         If naming it is genuinely necessary and nothing is being \
                         done to it, mark the line `// guard-allow: <reason>`.",
                        file.display(),
                        line_no,
                        marker,
                        why,
                        line.trim()
                    );
                }
            }
        }
    }
}

/// The reMarkable 2 has no display server, no Chromium, and a modest ARM CPU.
/// A portable crate that reaches for the network or a UI toolkit on its own has
/// already made a decision that belongs to an adapter.
#[test]
fn the_domain_core_has_no_io_dependencies_at_all() {
    let core = read_manifests()
        .into_iter()
        .find(|c| c.name == "marginalia-core")
        .expect("core");

    let io_shaped = ["rusqlite", "reqwest", "hyper", "tokio", "std-fs", "ureq"];
    for dep in &io_shaped {
        assert!(
            !core.external_deps.contains(*dep),
            "marginalia-core depends on '{dep}'. The core is pure: no filesystem, \
             no network, no database. That purity is what lets the safety rules be \
             tested exhaustively without a device."
        );
    }
}

/// Guards the roadmap's rule that expensive or unavailable capabilities are
/// selected through explicit interfaces, not scattered `cfg(target)` checks.
#[test]
fn domain_and_application_crates_do_not_branch_on_the_target_platform() {
    for krate in read_manifests() {
        if !is_platform_agnostic(&krate.name) {
            continue;
        }
        for file in rust_sources(&krate.dir) {
            let source = fs::read_to_string(&file).expect("read source");
            for (line_no, line) in code_lines(&source) {
                let branches_on_os = line.contains("cfg(target_os")
                    || line.contains("cfg(target_arch")
                    || line.contains("cfg(windows")
                    || line.contains("cfg(unix");
                assert!(
                    !branches_on_os,
                    "{}:{} branches on the target platform.\n  {}\n\
                     Platform differences belong behind a port, implemented in \
                     an adapter crate (see ADAPTER_CRATES), not sprinkled \
                     through domain logic.",
                    file.display(),
                    line_no,
                    line.trim()
                );
            }
        }
    }
}

// ── the device tooling ──────────────────────────────────────────────────────

/// Shell scripts that talk to someone's reMarkable over SSH are exactly where a
/// careless line does damage, and they are not covered by the Rust rules above.
///
/// This is not a security boundary — a determined edit defeats it. It is a
/// guard against drift: the day someone reaches for `systemctl` to make the
/// agent start automatically, this fails and the conversation happens in
/// review rather than on a user's device.
#[test]
fn the_device_scripts_contain_no_forbidden_commands() {
    let tools = workspace_root().join("tools/device");
    let Ok(entries) = fs::read_dir(&tools) else {
        return; // the tooling is optional; absence is not a failure
    };

    // Each of these would mean touching the device's own software.
    let forbidden = [
        ("systemctl", "creating or changing a system service"),
        ("/etc/init.d", "an init script"),
        ("opkg", "the system package manager"),
        ("toltec", "a third-party package manager"),
        ("LD_PRELOAD", "injecting into a running process"),
        ("xochitl", "the reMarkable's own application"),
    ];

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|e| e != "sh") {
            continue;
        }
        let source = fs::read_to_string(&path).expect("read script");

        for (line_no, line) in source.lines().enumerate() {
            let trimmed = line.trim_start();
            // Comments may name these; explaining what we never do is the point
            // of half that documentation. So may output lines: `doctor.sh`
            // prints "never: xochitl, the kernel, ..." precisely to tell the
            // user what Marginalia does not do. What matters is whether the
            // script *runs* one of these, not whether it says the word.
            // `# guard-allow: <reason>` marks a line that names a forbidden
            // thing in order to check for its *absence* — reset.sh looks for
            // system services precisely so that finding one stops the script.
            // The exception has to be written down, which is the point.
            if trimmed.starts_with('#')
                || is_output_line(trimmed)
                || line.contains("# guard-allow:")
            {
                continue;
            }
            for (marker, why) in &forbidden {
                assert!(
                    !line.contains(marker),
                    "{}:{} uses '{}' ({}) outside a comment.\n  {}\n\
                     If this is genuinely needed, it is a change to \
                     docs/safety/DEVICE_WRITE_POLICY.md first.",
                    path.display(),
                    line_no + 1,
                    marker,
                    why,
                    trimmed
                );
            }
        }
    }
}

/// Whether a line only prints text, rather than running a command.
fn is_output_line(trimmed: &str) -> bool {
    const OUTPUT_HELPERS: &[&str] = &[
        "say", "info", "warn", "ok", "fail", "step", "die", "printf", "echo",
    ];
    let first_word = trimmed
        .split(|c: char| c.is_whitespace() || c == '(')
        .next()
        .unwrap_or("");
    OUTPUT_HELPERS.contains(&first_word)
}

/// Both the installer and the reset script must call the guard that keeps every
/// write inside Marginalia's own directory. A script that skips it is a script
/// that can be pointed at `/`.
#[test]
fn the_device_scripts_assert_their_home_is_safe() {
    let tools = workspace_root().join("tools/device");
    for script in ["install.sh", "reset.sh", "doctor.sh"] {
        let path = tools.join(script);
        if !path.exists() {
            continue;
        }
        let source = fs::read_to_string(&path).expect("read script");
        assert!(
            source.contains("assert_home_is_safe"),
            "{script} does not call assert_home_is_safe. Without it, a mistyped \
             MARGINALIA_HOME makes the script dangerous."
        );
    }
}

/// Crates that are portable but deliberately absent from `cross-check`.
///
/// `cargo check --target armv7` cannot build these without a cross **C**
/// compiler, which the container build (`make build-device-docker`) has and a
/// bare cargo does not. They are still built for the device — just not by this
/// particular fast check.
const CROSS_CHECK_EXEMPT: &[(&str, &str)] = &[
    (
        "marginalia-database",
        "libsqlite3-sys compiles SQLite from C; covered by make build-device-docker",
    ),
    (
        "marginalia-sync",
        "depends on marginalia-database, so it inherits the same C toolchain need",
    ),
];

/// Every crate the tests call portable must also be in the Makefile's
/// cross-compile list, unless it is exempt above.
///
/// This exists because the two drifted: `marginalia-library-folder` was added,
/// declared portable here, and left out of `make cross-check` — so CI would
/// have gone on passing while a crate quietly stopped building for the device.
/// A list maintained in two places by memory is a list that diverges.
#[test]
fn the_cross_compile_list_covers_every_portable_crate() {
    let makefile = fs::read_to_string(workspace_root().join("Makefile")).expect("read Makefile");

    // The PORTABLE variable, which may be continued across lines with a `\`.
    let start = makefile
        .find("PORTABLE")
        .expect("Makefile defines PORTABLE");
    let mut block = String::new();
    for line in makefile[start..].lines() {
        block.push_str(line);
        if !line.trim_end().ends_with('\\') {
            break;
        }
    }

    for krate in PORTABLE_CRATES {
        if CROSS_CHECK_EXEMPT.iter().any(|(name, _)| name == krate) {
            continue;
        }
        assert!(
            block.contains(krate),
            "{krate} is declared portable but is missing from the Makefile's \
             PORTABLE list, so `make cross-check` would not build it for the \
             reMarkable.\n\nPORTABLE currently reads:\n{block}"
        );
    }
}

/// The justfile mirrors the Makefile, so it has to carry the same list.
#[test]
fn the_justfile_mirrors_the_cross_compile_list() {
    let justfile = fs::read_to_string(workspace_root().join("justfile")).expect("read justfile");
    let line = justfile
        .lines()
        .find(|l| l.starts_with("PORTABLE"))
        .expect("justfile defines PORTABLE");

    for krate in PORTABLE_CRATES {
        if CROSS_CHECK_EXEMPT.iter().any(|(name, _)| name == krate) {
            continue;
        }
        assert!(
            line.contains(krate),
            "{krate} is missing from the justfile's PORTABLE list. The two task \
             runners must agree, or `just check` and `make check` verify \
             different things."
        );
    }
}
