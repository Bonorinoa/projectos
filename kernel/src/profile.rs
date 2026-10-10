//! Installation profile registry — implemented by child P per `INTERFACES-M1b.md`.
//!
//! The profile graph is the **installation-specific** half of the federated
//! project/model split (brief §15). It records, per project UUID, a
//! machine-local `path_binding` (never identity), the last-seen checkpoint,
//! and cross-project links. It lives in an app-data directory the caller
//! passes, and it is backed by a single TOML file: `<app_data>/profile.toml`.
//!
//! Authority rules in force here (frozen):
//! - A path is a **local binding**, never project identity. Identity is the
//!   on-disk sidecar UUID (`.projectos/project.toml`, see `sidecar.rs`).
//! - The profile graph must **never override a project UUID**. During
//!   `restore`, whenever an entry's folder holds a different sidecar UUID, the
//!   **SIDECAR wins** and the entry is re-bound to the on-disk project.
//! - Ordering is deterministic: `entries()` yields entries sorted by their
//!   project UUID string, so exports and round-trips are byte-stable.
//!
//! `uuid` is a dependency *without* its `serde` feature (Cargo.toml is frozen
//! and must not be edited), so `Uuid` is bridged to/from a string through a
//! serialized DTO rather than via `derive`, exactly like `sidecar.rs` does.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::error::Result;
use crate::sidecar::SidecarEnvelope;

/// Default file name of the persisted profile registry inside the app-data dir.
pub const PROFILE_FILE: &str = "profile.toml";

/// A cross-project link stored in the profile graph. Links are canonical only
/// here (brief §15); a project graph may hold non-authoritative cached stubs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileLink {
    /// Human-readable label for the link.
    pub label: String,
    /// The target object id in some project graph.
    pub target_object: String,
}

/// One profile entry: the installation's local knowledge about a project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileEntry {
    /// The project's canonical UUID (mirrors the on-disk sidecar).
    pub project_uuid: uuid::Uuid,
    /// Machine-local path binding; never identity.
    pub path_binding: PathBuf,
    /// The last checkpoint observed for this project, if any.
    pub last_seen_checkpoint: Option<String>,
    /// Cross-project links (canonical in the profile graph).
    pub links: Vec<ProfileLink>,
}

/// Outcome of a `restore` into a clean app-data dir.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestoreReport {
    /// Number of entries actually restored into the registry (folders present).
    pub entries: usize,
    /// Number of entries whose UUID was corrected to the on-disk sidecar UUID
    /// (the sidecar won a mismatch). Paths are re-bound for all restored
    /// entries; `rebound` counts the identity corrections.
    pub rebound: usize,
    /// UUIDs of entries whose folder was not present under `project_root`.
    pub missing: Vec<uuid::Uuid>,
}

/// The profile registry, loaded from `<app_data>/profile.toml`.
///
/// Entries are kept internally sorted by project-UUID string so `entries()`
/// and persisted output are deterministic.
#[derive(Debug, Clone)]
pub struct ProfileRegistry {
    app_data_dir: PathBuf,
    // Keyed by uuid string for deterministic ordering and stable get/upsert.
    entries: BTreeMap<String, ProfileEntry>,
}

impl ProfileRegistry {
    /// Open (creating if needed) the registry at `<app_data>/profile.toml`.
    ///
    /// A missing file yields an empty registry; a malformed file is an error
    /// (`KernelError::Io`/`InvalidSidecar` via the DTO parse).
    pub fn open(app_data_dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(app_data_dir)?;
        let path = app_data_dir.join(PROFILE_FILE);
        let mut entries = BTreeMap::new();
        if path.exists() {
            let content = std::fs::read_to_string(&path)?;
            let dto: RegistryDto =
                toml::from_str(&content).map_err(|e| crate::error::KernelError::InvalidSidecar(
                    format!("malformed profile.toml: {e}"),
                ))?;
            for edto in dto.entries {
                let entry = ProfileEntry::try_from(edto)?;
                entries.insert(entry.project_uuid.to_string(), entry);
            }
        }
        Ok(Self { app_data_dir: app_data_dir.to_path_buf(), entries })
    }

    /// Insert or replace the profile entry for `entry.project_uuid`.
    pub fn upsert(&mut self, entry: ProfileEntry) -> Result<()> {
        self.entries.insert(entry.project_uuid.to_string(), entry);
        Ok(())
    }

    /// Look up an entry by project UUID.
    pub fn get(&self, uuid: &uuid::Uuid) -> Option<&ProfileEntry> {
        self.entries.get(&uuid.to_string())
    }

    /// All entries in deterministic (project-UUID-sorted) order.
    pub fn entries(&self) -> Vec<&ProfileEntry> {
        self.entries.values().collect()
    }

    /// Atomically persist the registry to `<app_data>/profile.toml`
    /// (tmp + rename, same pattern as the sidecar writer).
    pub fn save(&self) -> Result<()> {
        let path = self.app_data_dir.join(PROFILE_FILE);
        write_atomic(&path, &self.registry_toml())
    }

    /// Export the registry to an arbitrary `to` path (atomic TOML write).
    ///
    /// Absolute path bindings remain stored (they are local bindings, and
    /// `restore` re-binds them under a new `project_root`; they are never
    /// exported as identity).
    pub fn export(&self, to: &Path) -> Result<()> {
        write_atomic(to, &self.registry_toml())
    }

    /// Restore from an exported profile file into a clean app-data dir.
    ///
    /// Each exported entry is re-bound to `project_root/<basename of its old
    /// path binding>`, and the folder's sidecar UUID is verified:
    /// - folder present + sidecar UUID matches → entry kept (binding re-bound).
    /// - folder present + sidecar UUID differs → **the sidecar UUID wins**;
    ///   the entry is re-keyed/rebound to the on-disk project (`rebound += 1`).
    /// - folder absent (or sidecar unreadable) → UUID lands in `report.missing`.
    ///
    /// The restored set replaces this registry's entries.
    pub fn restore(
        &mut self,
        from: &Path,
        project_root: &Path,
    ) -> Result<RestoreReport> {
        let content = std::fs::read_to_string(from)?;
        let dto: RegistryDto =
            toml::from_str(&content).map_err(|e| crate::error::KernelError::InvalidSidecar(
                format!("malformed exported profile: {e}"),
            ))?;
        let exported: Vec<ProfileEntry> = dto
            .entries
            .into_iter()
            .map(ProfileEntry::try_from)
            .collect::<Result<_>>()?;

        let mut restored = BTreeMap::new();
        let mut rebound = 0usize;
        let mut missing = Vec::new();

        for entry in exported {
            let base = entry.path_binding.file_name().map(|s| s.to_os_string());
            let folder = match base {
                Some(name) => project_root.join(name),
                None => {
                    missing.push(entry.project_uuid);
                    continue;
                }
            };
            match SidecarEnvelope::read(&folder) {
                Ok(Some(env)) => {
                    let mut kept = entry.clone();
                    kept.path_binding = folder;
                    if env.project_uuid != kept.project_uuid {
                        // The on-disk sidecar is the authority: the SIDECAR wins.
                        kept.project_uuid = env.project_uuid;
                        rebound += 1;
                    }
                    restored.insert(kept.project_uuid.to_string(), kept);
                }
                // Folder absent (sidecar.read -> None) or unreadable/invalid
                // sidecar: cannot verify identity, so report it missing.
                _ => missing.push(entry.project_uuid),
            }
        }

        let entries = restored.len();
        self.entries = restored;
        Ok(RestoreReport { entries, rebound, missing })
    }

    /// Serialize the whole registry to a canonical TOML string.
    fn registry_toml(&self) -> String {
        let dto = RegistryDto {
            entries: self.entries.values().map(EntryDto::from).collect(),
        };
        toml::to_string(&dto)
            .expect("serializing the profile registry is infallible")
    }
}

/// Serialize the registry to TOML at `path` atomically (tmp + rename).
fn write_atomic(path: &Path, toml_text: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("toml.tmp");
    std::fs::write(&tmp, toml_text)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

// --- Mapping between the public types and their serializable DTO forms. ---
//
// `Uuid` cannot derive serde here (uuid crate without the `serde` feature), so
// every string-typed field is bridged by explicit conversion, mirroring
// `sidecar.rs`.

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct RegistryDto {
    entries: Vec<EntryDto>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct EntryDto {
    project_uuid: String,
    path_binding: String,
    last_seen_checkpoint: Option<String>,
    links: Vec<LinkDto>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct LinkDto {
    label: String,
    target_object: String,
}

impl From<&ProfileLink> for LinkDto {
    fn from(l: &ProfileLink) -> Self {
        Self { label: l.label.clone(), target_object: l.target_object.clone() }
    }
}

impl From<LinkDto> for ProfileLink {
    fn from(d: LinkDto) -> Self {
        Self { label: d.label, target_object: d.target_object }
    }
}

impl From<&ProfileEntry> for EntryDto {
    fn from(e: &ProfileEntry) -> Self {
        Self {
            project_uuid: e.project_uuid.to_string(),
            path_binding: e.path_binding.to_string_lossy().into_owned(),
            last_seen_checkpoint: e.last_seen_checkpoint.clone(),
            links: e.links.iter().map(LinkDto::from).collect(),
        }
    }
}

impl TryFrom<EntryDto> for ProfileEntry {
    type Error = crate::error::KernelError;

    fn try_from(d: EntryDto) -> std::result::Result<Self, Self::Error> {
        let project_uuid = uuid::Uuid::parse_str(&d.project_uuid).map_err(|e| {
            crate::error::KernelError::InvalidSidecar(format!(
                "profile entry has invalid project_uuid: {e}"
            ))
        })?;
        Ok(Self {
            project_uuid,
            path_binding: PathBuf::from(d.path_binding),
            last_seen_checkpoint: d.last_seen_checkpoint,
            links: d.links.into_iter().map(ProfileLink::from).collect(),
        })
    }
}
