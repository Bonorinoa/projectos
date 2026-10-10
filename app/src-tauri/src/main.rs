//! ProjectOS desktop app shell — Tauri v2 entry point.
//!
//! Exposes three kernel-backed commands to the UI:
//!   - `project_status(folder)`  -> reads the `.projectos` sidecar envelope
//!   - `project_register(folder, declared_type, content_roots)` -> registers
//!   - `project_open(folder)`    -> opens a registered project (writer lock)
//!
//! All three go through the real `projectos_kernel` crate (no stubs): they use
//! `projectos_kernel::registration::{register, open}` and
//! `sidecar::SidecarEnvelope::read`. Errors are surfaced as strings.

use std::path::Path;

use projectos_kernel::registration;
use projectos_kernel::sidecar::SidecarEnvelope;

/// Serializable result for `project_open` (folder + envelope).
#[derive(serde::Serialize)]
struct OpenResult {
    folder: String,
    envelope: SidecarEnvelope,
}

/// Read the `.projectos/project.toml` sidecar for `folder`.
///
/// `None` means the folder is not yet registered. Returns a descriptive error
/// string when the sidecar is malformed or the schema is unsupported.
#[tauri::command]
fn project_status(folder: String) -> Result<Option<SidecarEnvelope>, String> {
    SidecarEnvelope::read(Path::new(&folder)).map_err(|e| e.to_string())
}

/// Register `folder` as a project of type `declared_type`.
///
/// `content_roots` are optional relative content roots (default: empty).
/// Idempotent for the same path; an alias of an already-registered project
/// surfaces `AliasPath` as an error string.
#[tauri::command]
fn project_register(
    folder: String,
    declared_type: String,
    content_roots: Option<Vec<String>>,
) -> Result<SidecarEnvelope, String> {
    let roots = content_roots.unwrap_or_default();
    let proj = registration::register(Path::new(&folder), &declared_type, roots)
        .map_err(|e| e.to_string())?;
    // RegisteredProject implements Drop (releases the lock), so clone out.
    Ok(proj.envelope.clone())
}

/// Open `folder` for a writer session (returns the envelope, holds the lock).
#[tauri::command]
fn project_open(folder: String) -> Result<OpenResult, String> {
    let proj = registration::open(Path::new(&folder)).map_err(|e| e.to_string())?;
    Ok(OpenResult {
        folder: proj.folder.to_string_lossy().to_string(),
        envelope: proj.envelope.clone(),
    })
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            project_status,
            project_register,
            project_open
        ])
        .run(tauri::generate_context!())
        .expect("error while running ProjectOS Tauri application");
}
