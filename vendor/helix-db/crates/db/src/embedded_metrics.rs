//! Stand-ins for the stripped upstream `helix-metrics` crate.
//!
//! ProjectOS's vendored fork (`vendor/helix-db/VENDOR.md`) removes the upstream
//! `helix-metrics` dependency, which is licensed AGPL-3.0 and therefore
//! incompatible with the product's locked Apache-2.0 licensing (decision D-22;
//! Gate B hard-fail screen). Embedded/server query telemetry is permanently
//! disabled in this fork. These local types preserve the signatures that
//! previously referenced the upstream metrics types so the rest of the crate is
//! unchanged; none of them has behaviour.
#![allow(dead_code)]

/// Replaces `helix_metrics::query::transport::OssQueryMetrics`.
///
/// The metrics transport no longer exists; a value of this type is inert.
#[derive(Clone, Default)]
pub struct OssQueryMetrics;

/// Replaces `helix_metrics::telemetry::Runtime`.
pub struct Runtime;

/// Replaces `helix_metrics::query::TenantId` (telemetry tenant identifier).
#[derive(Clone)]
pub struct TenantId;

/// Replaces `helix_metrics::query::QueryErrorType`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueryErrorType {
    Conflict,
    InvalidRequest,
    Planning,
    Execution,
    Internal,
}