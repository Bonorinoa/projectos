//! Provider abstraction + an offline deterministic mock (M2 core).
//!
//! This module is the **seam** where the model layer attaches. In M2 the only
//! implementation is [`MockProvider`], which is **deterministic, offline, and
//! credential-free** — it lets the whole Substack loop (action → proposal →
//! diff → approval → revision → receipt) be proven without a network or a key.
//!
//! The live adapter (Gate E: credential isolation, OS keyring, PKCE, streaming,
//! fallback, cost ceilings) is **M2.2 and is blocked on owner resources**. No
//! claim of live-provider behaviour is made here; see `docs/M2-PLAN-REVIEW.md`.
//!
//! ## Hash
//!
//! [`stable_hash`] is **FNV-1a 64-bit** rendered as 16 lowercase hex digits —
//! the same documented, non-cryptographic identity digest used by
//! `receipts.rs`. It guarantees determinism across identical inputs; it is not
//! a content-authenticity claim.

use crate::error::Result;

/// A typed model request passed to a [`Provider`].
#[derive(Debug, Clone, PartialEq)]
pub struct ProviderRequest {
    /// The requested model id (recorded in receipts as the *requested* route).
    pub model: String,
    /// The fully-assembled prompt (instruction + selected content).
    pub prompt: String,
}

/// A normalized model response.
#[derive(Debug, Clone, PartialEq)]
pub struct ProviderResponse {
    pub text: String,
    /// The provider-reported resolved model (equal to the request in the mock).
    pub resolved_model: String,
    pub usage_tokens: Option<u64>,
}

/// The model-provider contract. Implementations translate a typed request and
/// normalize the response; they must not mutate files or change permissions.
pub trait Provider {
    fn complete(&self, req: &ProviderRequest) -> Result<ProviderResponse>;
}

/// Deterministic, offline stand-in for a real provider.
///
/// Rule (documented): `text = "[mock:{model}] {prompt}"`, `resolved_model =
/// model`, `usage_tokens = Some(prompt.len())`. The same request always yields
/// the same response — no clock, no randomness, no network, no credentials.
#[derive(Debug, Default, Clone, Copy)]
pub struct MockProvider;

impl MockProvider {
    pub fn new() -> Self {
        MockProvider
    }
}

impl Provider for MockProvider {
    fn complete(&self, req: &ProviderRequest) -> Result<ProviderResponse> {
        Ok(ProviderResponse {
            text: format!("[mock:{}] {}", req.model, req.prompt),
            resolved_model: req.model.clone(),
            usage_tokens: Some(req.prompt.len() as u64),
        })
    }
}

/// FNV-1a 64-bit hash of `bytes` → 16 lowercase hex digits.
///
/// Deterministic identity digest (non-cryptographic), matching the convention
/// documented in `receipts.rs`.
pub fn stable_hash(bytes: &[u8]) -> String {
    let mut state: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        state ^= u64::from(b);
        state = state.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{state:016x}")
}
