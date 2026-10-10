//! Folder registration, identity and second-writer locking — implemented by
//! child A per `INTERFACES.md`.
//!
//! Identity is the **canonicalized realpath** (`std::fs::canonicalize`),
//! never the lexical path the caller passed. This slice runs in-process with no
//! persistent profile graph yet, so the set of *known* registrations is kept in
//! a process-local map keyed by canonical path. The map records the exact
//! `registered_path` the owner first used so an `AliasPath` error can name it.
//!
//! Disk is the authority for UUIDs: a sidecar already present on disk (e.g.
//! from a prior process) is re-read so the identity is stable across reopens,
//! and a future `schema_version` is rejected, never downgraded.
//!
//! The `.projectos/lock` file models a *writer session*: only [`open`] creates
//! it (via `create_new(true)`), and it is released when the returned
//! [`RegisteredProject`] handle is dropped. [`register`] and [`move_project`]
//! are not long-lived sessions; they perform an atomic sidecar write and do not
//! hold the lock, so re-registering or reopening never deadlocks on a stale
//! session.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use crate::error::{KernelError, Result, SCHEMA_VERSION};
use crate::sidecar::{check_roots_within, lock_path, SidecarEnvelope};

/// Versions stamped into a freshly written manifest by this slice.
const ENGINE_VERSION: &str = "0.0.1";
const EXPORT_VERSION: &str = "0.0.1";

/// A handle to a project opened for work.
///
/// `folder` is the path the caller used (unchanged). `envelope` is the sidecar
/// as read from disk. A handle produced by [`open`] holds the writer lock;
/// dropping it releases `.projectos/lock`. Handles produced by [`register`]
/// and [`move_project`] do not hold the lock.
#[derive(Debug)]
pub struct RegisteredProject {
    pub folder: PathBuf,
    pub envelope: SidecarEnvelope,
    holds_lock: bool,
}

impl Drop for RegisteredProject {
    fn drop(&mut self) {
        if self.holds_lock {
            // Best-effort release: failure is non-fatal and has no caller.
            let _ = std::fs::remove_file(lock_path(&self.folder));
        }
    }
}

/// In-memory record of a known registration, keyed by canonical path.
#[derive(Clone)]
struct RegistrationRecord {
    /// The exact path the owner first passed to `register`. Returned verbatim
    /// in an `AliasPath` error so it names the existing project.
    registered_path: PathBuf,
    uuid: uuid::Uuid,
}

/// Process-local registry of canonical-path → registration record.
fn registry() -> &'static Mutex<HashMap<PathBuf, RegistrationRecord>> {
    static REGISTRY: OnceLock<Mutex<HashMap<PathBuf, RegistrationRecord>>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Canonicalize `path`, resolving symlinks and intermediate `..`.
fn canonicalize(path: &Path) -> Result<PathBuf> {
    Ok(std::fs::canonicalize(path)?)
}

/// `true` if `path` itself is a symbolic link (does not follow it).
fn is_symlink(path: &Path) -> bool {
    std::fs::symlink_metadata(path)
        .map(|m| m.is_symlink())
        .unwrap_or(false)
}

/// Register an existing project folder.
///
/// * Already registered under the **same** path → idempotent: the existing
///   sidecar is read and returned with its stable UUID (no rewrite, no new
///   UUID, no error).
/// * Reached through a **different** path that canonicalizes to an
///   already-registered project (symlink / case-insensitive alias) →
///   [`KernelError::AliasPath`] naming the original registration path.
/// * Otherwise a new UUID is generated and the sidecar is written atomically.
pub fn register(
    folder: &Path,
    declared_type: &str,
    content_roots: Vec<String>,
) -> Result<RegisteredProject> {
    let real = canonicalize(folder)?;
    let existing = SidecarEnvelope::read(&real)?;

    let reg = registry().lock().unwrap();
    if let Some(record) = reg.get(&real) {
        if &record.registered_path != folder {
            // Same project, different path: an alias.
            return Err(KernelError::AliasPath(
                record.registered_path.to_string_lossy().to_string(),
            ));
        }
        // Idempotent re-registration of the same path: keep the existing UUID.
        let env = existing.ok_or_else(|| {
            KernelError::NotRegistered(real.to_string_lossy().to_string())
        })?;
        return Ok(RegisteredProject {
            folder: folder.to_path_buf(),
            envelope: env,
            holds_lock: false,
        });
    }
    drop(reg);

    // Not in the in-memory registry. Is there a sidecar on disk from a prior
    // process? If so, treat it as already registered: idempotent unless the
    // input is a symlink alias.
    if let Some(env) = existing {
        if is_symlink(folder) {
            return Err(KernelError::AliasPath(
                real.to_string_lossy().to_string(),
            ));
        }
        registry().lock().unwrap().insert(
            real.clone(),
            RegistrationRecord {
                registered_path: folder.to_path_buf(),
                uuid: env.project_uuid,
            },
        );
        return Ok(RegisteredProject {
            folder: folder.to_path_buf(),
            envelope: env,
            holds_lock: false,
        });
    }

    // Truly new registration.
    check_roots_within(&real, &content_roots)?;
    let project_uuid = uuid::Uuid::new_v4();
    let env = SidecarEnvelope {
        project_uuid,
        declared_type: declared_type.to_string(),
        content_roots,
        schema_version: SCHEMA_VERSION,
        engine_version: ENGINE_VERSION.to_string(),
        export_version: EXPORT_VERSION.to_string(),
        checkpoint: None,
    };
    SidecarEnvelope::write(&real, &env)?;

    registry().lock().unwrap().insert(
        real,
        RegistrationRecord {
            registered_path: folder.to_path_buf(),
            uuid: project_uuid,
        },
    );

    Ok(RegisteredProject {
        folder: folder.to_path_buf(),
        envelope: env,
        holds_lock: false,
    })
}

/// Open a registered project for a writer session.
///
/// Creates `.projectos/lock` via `create_new(true)`; a second concurrent
/// `open` on the same folder fails with [`KernelError::ProjectLocked`]. The
/// lock is released when the returned [`RegisteredProject`] is dropped.
pub fn open(folder: &Path) -> Result<RegisteredProject> {
    let env = match SidecarEnvelope::read(folder)? {
        Some(env) => env,
        None => {
            return Err(KernelError::NotRegistered(
                folder.to_string_lossy().to_string(),
            ))
        }
    };

    match std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(lock_path(folder))
    {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            return Err(KernelError::ProjectLocked(
                folder.to_string_lossy().to_string(),
            ));
        }
        Err(e) => return Err(KernelError::from(e)),
    }

    Ok(RegisteredProject {
        folder: folder.to_path_buf(),
        envelope: env,
        holds_lock: true,
    })
}

/// Move a project folder to a new path, preserving its UUID.
///
/// The `.projectos` sidecar travels with the folder; nothing inside
/// `project.toml` is rewritten. The canonical-path registry is repointed to the
/// new location. `from` must already be registered and `to` must not exist.
pub fn move_project(from: &Path, to: &Path) -> Result<RegisteredProject> {
    let env = match SidecarEnvelope::read(from)? {
        Some(env) => env,
        None => {
            return Err(KernelError::NotRegistered(
                from.to_string_lossy().to_string(),
            ))
        }
    };

    if to.exists() {
        return Err(KernelError::InvalidSidecar(format!(
            "destination already exists: {}",
            to.to_string_lossy()
        )));
    }

    // Capture the source's canonical path while it still exists; after the
    // rename the old path is gone and cannot be re-canonicalized.
    let real_from = canonicalize(from)?;
    move_dir(from, to)?;
    let real_to = canonicalize(to)?;

    // Repoint the registry entry from the old canonical path to the new one.
    let mut reg = registry().lock().unwrap();
    if let Some(record) = reg.remove(&real_from) {
        reg.insert(
            real_to,
            RegistrationRecord {
                registered_path: to.to_path_buf(),
                uuid: record.uuid,
            },
        );
    }

    Ok(RegisteredProject {
        folder: to.to_path_buf(),
        envelope: env,
        holds_lock: false,
    })
}

/// Move `from` to `to`, falling back to a copy for cross-device renames.
fn move_dir(from: &Path, to: &Path) -> Result<()> {
    match std::fs::rename(from, to) {
        Ok(()) => return Ok(()),
        // EXDEV (cross-device link): retry with a copy + delete.
        Err(e) if e.raw_os_error() == Some(18) => {}
        Err(e) => return Err(e.into()),
    }
    copy_dir_recursive(from, to)?;
    std::fs::remove_dir_all(from)?;
    Ok(())
}

fn copy_dir_recursive(from: &Path, to: &Path) -> Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let name = entry.file_name();
        let src = entry.path();
        let dst = to.join(&name);
        let t = entry.file_type()?;
        if t.is_dir() && !t.is_symlink() {
            copy_dir_recursive(&src, &dst)?;
        } else {
            std::fs::copy(&src, &dst)?;
        }
    }
    Ok(())
}

/// Return `Some(existing_path)` if `folder` is an alias (symlink or
/// case-insensitive duplicate) of an already-registered project.
///
/// The folder is resolved to its canonical realpath and looked up in the
/// in-memory registry; if the caller reached the same project through a
/// different path, the originally registered path is returned.
pub fn is_alias_of_registered(folder: &Path) -> Result<Option<PathBuf>> {
    let real = match canonicalize(folder) {
        Ok(r) => r,
        Err(_) => return Ok(None),
    };
    let reg = registry().lock().unwrap();
    if let Some(record) = reg.get(&real) {
        if &record.registered_path != folder {
            return Ok(Some(record.registered_path.clone()));
        }
    }
    drop(reg);
    // Fallback: a sidecar on disk from a prior process.
    if SidecarEnvelope::read(folder)?.is_some() && is_symlink(folder) {
        return Ok(Some(real));
    }
    Ok(None)
}
