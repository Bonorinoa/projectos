//! `.projectos/project.toml` sidecar envelope — implemented by child A per
//! `INTERFACES.md`.
//!
//! The manifest file is the canonical store of a project's portable identity
//! (UUID), its declared type, and its compatibility versions. It travels with
//! the folder and contains no secrets or machine-absolute identity.
//!
//! `uuid` is a dependency *without* its `serde` feature (Cargo.toml is frozen
//! and we must not edit it), so `Uuid` is bridged to/from a string through a
//! serialized DTO rather than via `derive`. This keeps the public struct fields
//! (`project_uuid: uuid::Uuid`) exactly as the contract specifies while using
//! only the crates that are present in `Cargo.toml`.

use std::path::{Component, Path, PathBuf};

pub use crate::error::SCHEMA_VERSION;
use crate::error::{KernelError, Result};

/// Name of the sidecar directory that lives inside every registered project.
pub const SIDECAR_DIR: &str = ".projectos";
/// Name of the manifest file within [`SIDECAR_DIR`].
pub const MANIFEST_FILE: &str = "project.toml";

/// The envelope written to `.projectos/project.toml`.
///
/// This is the portable, human-auditable half of project identity. The
/// `project_uuid` field *owns* the identity of the project (per brief §15);
/// the path is a local binding, never identity.
#[derive(Debug, Clone, PartialEq)]
pub struct SidecarEnvelope {
    pub project_uuid: uuid::Uuid,
    pub declared_type: String,
    pub content_roots: Vec<String>,
    pub schema_version: u32,
    pub engine_version: String,
    pub export_version: String,
    pub checkpoint: Option<String>,
}

/// Serialized form of [`SidecarEnvelope`] used only for (de)serialization.
///
/// `project_uuid` is modeled as a string here because the `uuid` crate in this
/// build does not enable its `serde` feature. The (de)serialization of
/// [`SidecarEnvelope`] routes through this DTO.
#[derive(serde::Serialize, serde::Deserialize)]
struct EnvelopeDto {
    project_uuid: String,
    declared_type: String,
    content_roots: Vec<String>,
    schema_version: u32,
    engine_version: String,
    export_version: String,
    checkpoint: Option<String>,
}

impl From<&SidecarEnvelope> for EnvelopeDto {
    fn from(env: &SidecarEnvelope) -> Self {
        Self {
            project_uuid: env.project_uuid.to_string(),
            declared_type: env.declared_type.clone(),
            content_roots: env.content_roots.clone(),
            schema_version: env.schema_version,
            engine_version: env.engine_version.clone(),
            export_version: env.export_version.clone(),
            checkpoint: env.checkpoint.clone(),
        }
    }
}

impl TryFrom<EnvelopeDto> for SidecarEnvelope {
    type Error = KernelError;

    fn try_from(dto: EnvelopeDto) -> std::result::Result<Self, Self::Error> {
        let project_uuid = uuid::Uuid::parse_str(&dto.project_uuid).map_err(|e| {
            KernelError::InvalidSidecar(format!("invalid project_uuid: {e}"))
        })?;
        Ok(Self {
            project_uuid,
            declared_type: dto.declared_type,
            content_roots: dto.content_roots,
            schema_version: dto.schema_version,
            engine_version: dto.engine_version,
            export_version: dto.export_version,
            checkpoint: dto.checkpoint,
        })
    }
}

impl serde::Serialize for SidecarEnvelope {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        EnvelopeDto::from(self).serialize(serializer)
    }
}

impl<'de> serde::Deserialize<'de> for SidecarEnvelope {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let dto = EnvelopeDto::deserialize(deserializer)?;
        SidecarEnvelope::try_from(dto).map_err(serde::de::Error::custom)
    }
}

impl SidecarEnvelope {
    /// Read the sidecar from a project `folder`.
    ///
    /// Returns `Ok(None)` when no `.projectos/project.toml` exists (the
    /// project has not been registered). A sidecar whose `schema_version` is
    /// newer than [`SCHEMA_VERSION`] is rejected with
    /// [`KernelError::UnsupportedSchema`] — we never silently downgrade.
    pub fn read(folder: &Path) -> Result<Option<Self>> {
        let manifest = manifest_path(folder);
        if !manifest.exists() {
            return Ok(None);
        }
        let content = std::fs::read_to_string(&manifest)?;
        let env: SidecarEnvelope =
            toml::from_str(&content).map_err(|e| {
                KernelError::InvalidSidecar(format!("malformed manifest: {e}"))
            })?;
        // `validate` rejects future schemas with UnsupportedSchema (never a
        // silent downgrade) and enforces relative content roots.
        env.validate()?;
        Ok(Some(env))
    }

    /// Atomically write the envelope to the project `folder`.
    ///
    /// The file is first written to `project.toml.tmp` and then renamed
    /// over `project.toml` so a crash mid-write cannot leave a half-written
    /// manifest.
    pub fn write(folder: &Path, env: &Self) -> Result<()> {
        let dir = sidecar_dir(folder);
        std::fs::create_dir_all(&dir)?;
        env.validate()?;

        let manifest = manifest_path(folder);
        let tmp = dir.join(format!("{}.tmp", MANIFEST_FILE));
        let toml = toml::to_string(env).map_err(|e| {
            KernelError::InvalidSidecar(format!("cannot serialize manifest: {e}"))
        })?;
        std::fs::write(&tmp, toml)?;
        std::fs::rename(&tmp, &manifest)?;
        Ok(())
    }

    /// Validate structural invariants of the envelope.
    ///
    /// * `schema_version` must not exceed the supported version (future
    ///   schemas raise [`KernelError::UnsupportedSchema`], never a silent
    ///   downgrade).
    /// * `declared_type` must be non-empty.
    /// * Every `content_roots` entry must be a relative path that does not
    ///   escape the project folder (`..`, absolute paths, or a
    ///   symlink-resolved escape → [`KernelError::InvalidSidecar`]).
    pub fn validate(&self) -> Result<()> {
        if self.schema_version > SCHEMA_VERSION {
            return Err(KernelError::UnsupportedSchema {
                found: self.schema_version,
                expected: SCHEMA_VERSION,
            });
        }
        if self.declared_type.trim().is_empty() {
            return Err(KernelError::InvalidSidecar(
                "declared_type must not be empty".to_string(),
            ));
        }
        for root in &self.content_roots {
            validate_relative_root(root)?;
        }
        Ok(())
    }
}

/// Reject absolute paths, empty roots, and any `..` component.
///
/// A bare lexical check cannot detect a symlink that points outside the
/// folder, so callers that have a live folder also run
/// [`check_roots_within`] which canonicalizes existing roots.
fn validate_relative_root(root: &str) -> Result<()> {
    if root.is_empty() {
        return Err(KernelError::InvalidSidecar(
            "content root must not be empty".to_string(),
        ));
    }
    let path = Path::new(root);
    match path.components().next() {
        Some(Component::Prefix(_)) | Some(Component::RootDir) | Some(Component::CurDir) => {
            return Err(KernelError::InvalidSidecar(format!(
                "content root must be relative: {root}"
            )));
        }
        _ => {}
    }
    for component in path.components() {
        if matches!(component, Component::ParentDir) {
            return Err(KernelError::InvalidSidecar(format!(
                "content root escapes the project folder: {root}"
            )));
        }
    }
    Ok(())
}

/// Confirm a relative `root` resolves inside `folder`, rejecting symlink-based
/// escapes.
///
/// Only roots that actually exist on disk are canonicalized (a declared root
/// that does not yet exist is still accepted so a project can be registered
/// before its content is present).
pub(crate) fn check_roots_within(folder: &Path, roots: &[String]) -> Result<()> {
    let canonical_folder = canonical(folder)?;
    for root in roots {
        validate_relative_root(root)?;
        let joined = folder.join(root);
        if joined.exists() {
            let canonical_root = canonical(&joined)?;
            let within = canonical_root == canonical_folder
                || canonical_root.starts_with(&canonical_folder);
            if !within {
                return Err(KernelError::InvalidSidecar(format!(
                    "content root resolves outside the project folder: {root}"
                )));
            }
        }
    }
    Ok(())
}

/// Canonically resolve a path, returning the absolute realpath.
fn canonical(path: &Path) -> Result<PathBuf> {
    Ok(std::fs::canonicalize(path)?)
}

fn sidecar_dir(folder: &Path) -> PathBuf {
    folder.join(SIDECAR_DIR)
}

fn manifest_path(folder: &Path) -> PathBuf {
    sidecar_dir(folder).join(MANIFEST_FILE)
}

// The lock file lives at `.projectos/lock` inside the project folder. Exposed
// to the registration module so a single source of truth names the path.
pub(crate) fn lock_path(folder: &Path) -> PathBuf {
    sidecar_dir(folder).join("lock")
}
