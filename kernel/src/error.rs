//! Kernel error type — implemented by child A per `INTERFACES.md`.
//!
//! This covers only the registration + sidecar slice of the kernel. The
//! capability broker (`broker.rs`) has its own error handling and is owned by
//! child B.

/// A stable identifier for the current manifest schema. A sidecar carrying a
/// `schema_version` greater than this is rejected with [`KernelError::UnsupportedSchema`]
/// rather than silently downgraded.
pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, thiserror::Error)]
pub enum KernelError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    #[error("invalid sidecar: {0}")]
    InvalidSidecar(String),

    #[error("unsupported schema: found {found}, expected {expected}")]
    UnsupportedSchema { found: u32, expected: u32 },

    #[error("project is locked by another writer: {0}")]
    ProjectLocked(String),

    #[error("folder is an alias/duplicate of a registered project: {0}")]
    AliasPath(String),

    #[error("no project at {0}")]
    NotRegistered(String),
}

pub type Result<T> = std::result::Result<T, KernelError>;
