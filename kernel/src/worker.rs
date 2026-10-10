//! Worker host trait + in-process adapter.
//!
//! Child B slice per `INTERFACES.md`. This file implements:
//! - `WorkerHost`: the trait boundary a worker adapter must satisfy.
//! - `WorkerOp`: the operation envelope passed to a worker.
//! - `InProcessWorker`: the first implementation of `WorkerHost`.
//!
//! ## Status of this adapter
//!
//! `InProcessWorker` proves the **WorkerHost interface**, not confinement. Its
//! doc comment below states this plainly. It holds a cloned broker handle and
//! routes every `WorkerOp` through `Broker::request`, so its denials behave
//! identically to the broker's — but it makes no OS-level confinement claim.
//!
//! The **WASM guest adapter** (wasmtime/javy, Gate C2 evidence) is the second
//! implementation of `WorkerHost`. It is deferred until Gate C packaging
//! closes; the broker alone is usable evidence, not authorization to build
//! product features on an unproven worker boundary (see
//! `docs/adr-worker-isolation.md`).

use crate::error::Result;
use crate::broker::{Broker, BrokerOutcome, Receipt};

/// An operation presented to a worker: a single brokered request.
#[derive(Debug)]
pub struct WorkerOp {
    pub request: crate::broker::BrokerRequest,
}

/// Trait boundary for a worker host.
///
/// `start` primes the worker with a broker handle. `invoke` processes one
/// operation and must return the broker's outcome. `cancel` signals abort
/// (a documented no-op in the in-process adapter). `receipts` returns the
/// worker's accumulated redacted receipts.
pub trait WorkerHost {
    fn start(&mut self, broker: &mut Broker) -> Result<()>;
    fn invoke(&mut self, op: WorkerOp) -> Result<BrokerOutcome>;
    fn cancel(&mut self) -> Result<()>;
    fn receipts(&self) -> Vec<Receipt>;
}

/// In-process worker adapter — the first implementation of `WorkerHost`.
///
/// This adapter **proves the `WorkerHost` interface** (routing every op through
/// the broker so denials behave identically to the broker's), and makes **no
/// confinement claim**. It runs in the same process and address space as the
/// caller; it is not a security boundary.
///
/// The **WASM guest adapter** (wasmtime/javy, per Gate C2 evidence) is the
/// second implementation of `WorkerHost` and is deferred until Gate C packaging
/// closes. Do not build product features on this in-process adapter's boundary.
pub struct InProcessWorker {
    broker: Option<Broker>,
}

impl Default for InProcessWorker {
    fn default() -> Self {
        InProcessWorker { broker: None }
    }
}

impl InProcessWorker {
    pub fn new() -> Self {
        Self::default()
    }
}

impl WorkerHost for InProcessWorker {
    fn start(&mut self, broker: &mut Broker) -> Result<()> {
        // Clone the broker so the worker owns an independent handle for the
        // session. Grants and receipts are carried over (empty at start).
        self.broker = Some(Broker::new(
            broker.root().to_path_buf(),
            broker.grants().clone(),
        ));
        Ok(())
    }

    /// Every op routes through the broker; denials behave identically to the
    /// broker's because they *are* the broker's.
    fn invoke(&mut self, op: WorkerOp) -> Result<BrokerOutcome> {
        let broker = self
            .broker
            .as_mut()
            .expect("InProcessWorker::start must be called first");
        broker.request(op.request)
    }

    /// No-op success in this adapter (documented). The WASM guest will provide
    /// real cancellation; here there is nothing to interrupt.
    fn cancel(&mut self) -> Result<()> {
        // no-op success
        Ok(())
    }

    fn receipts(&self) -> Vec<Receipt> {
        self.broker
            .as_ref()
            .map(|b| b.receipts().to_vec())
            .unwrap_or_default()
    }
}
