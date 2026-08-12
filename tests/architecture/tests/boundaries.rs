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
];

/// Adapters: the layer whose *job* is to know about a host.
///
/// They must still cross-compile and must still avoid desktop-only
/// dependencies, but `#[cfg(unix)]` is legitimate here — that is the whole
/// point of an adapter. Everywhere else it is a platform assumption leaking
/// into logic that should not have one.
const ADAPTER_CRATES: &[&str] = &["marginalia-platform", "marginalia-zotero"];

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

    // Test-only crates may depend on anything they are testing.
    let everything: BTreeSet<&str> = PORTABLE_CRATES
        .iter()
        .copied()
        .chain(["marginalia-simulator"])
        .collect();
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
                for (marker, why) in FORBIDDEN_SOURCE_MARKERS {
                    assert!(
                        !line.contains(marker),
                        "{}:{} contains '{}' ({}) outside a comment.\n  {}",
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
