//! ProjectOS kernel — M1 thin vertical slice.
//!
//! Scope frozen 2026-10-09 (owner decisions): project registration, the
//! `.projectos/` sidecar envelope, and the capability broker behind a
//! `WorkerHost` trait. The `GraphEngine` trait and the profile graph are the
//! next slice; the WASM worker adapter is the second implementation of
//! `WorkerHost` and is deferred until Gate C packaging closes.
//!
//! Contract for this slice: see `INTERFACES.md`.
//! Product decisions: `docs/projectos-architecture-brief.md` (D-01..D-22).

pub mod broker;
pub mod error;
pub mod graph;
pub mod graph_memory;
pub mod journal;
pub mod profile;
pub mod receipts;
pub mod registration;
pub mod sidecar;
pub mod worker;
