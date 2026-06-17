//! `gauge-telemetry` — privacy-first telemetry client kernel for Gauge.
//!
//! Apps build a [`Telemetry`] handle once at startup, then `emit` typed events.
//! The hot path only appends one line to a disk queue; delivery happens out of
//! band. See `SPEC.md` for the wire contract.

pub mod canary;
pub mod client;
pub mod common;
pub mod consent;
pub mod env;
pub mod event;
pub mod flush;

pub(crate) mod identity;

pub use canary::{FORBIDDEN_SUBSTRINGS, assert_no_forbidden};
pub use client::{BuildError, Builder, Telemetry};
pub use flush::Flusher;

/// Crate version, stamped at build time.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
