//! The Tauri entry point.
//!
//! Phase 0 wires logging, opens the database, and exposes two read-only
//! commands. There is deliberately no device command surface yet: the transport
//! does not exist, and an IPC command that "will be safe once we add the
//! checks" is exactly the shape of mistake this project is built to avoid.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use marginalia_safety::{FeatureFlag, FeatureFlagManager};
use serde::Serialize;

#[derive(Serialize)]
struct FlagView {
    name: String,
    enabled: bool,
    experimental: bool,
}

/// The current feature-flag state, for the Settings screen.
#[tauri::command]
fn feature_flags() -> Vec<FlagView> {
    let flags = FeatureFlagManager::new();
    FeatureFlag::ALL
        .iter()
        .map(|flag| FlagView {
            name: format!("{flag:?}"),
            enabled: flags.is_enabled(*flag),
            experimental: flag.is_experimental(),
        })
        .collect()
}

/// What Marginalia is currently permitted to do. Read-only in Phase 0.
#[tauri::command]
fn safety_status() -> serde_json::Value {
    serde_json::json!({
        "safe_mode": true,
        "device_connected": false,
        "writes_enabled": false,
        "explanation": "No device transport is implemented yet. Marginalia \
                        cannot modify a reMarkable in this build."
    })
}

fn main() {
    marginalia_observability::init(false);

    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![feature_flags, safety_status])
        .run(tauri::generate_context!())
        .expect("error while running Marginalia");
}
