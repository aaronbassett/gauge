//! `gauge-telemetry` — privacy-first telemetry client kernel for Gauge.
//!
//! Apps build a [`Telemetry`] handle once at startup, then `emit` typed events.
//! The hot path only appends one line to a disk queue; delivery happens out of
//! band. All quantities are sent as raw integers (bucketed at read time);
//! attribute values are scalars only. See `SPEC.md` for the wire contract and
//! `PORTING.md` for migrating an existing app.
//!
//! ```ignore
//! use std::time::Duration;
//! use gauge_telemetry::Telemetry;
//! use gauge_telemetry::client::DEFAULT_FLUSH_TIMEOUT;
//! use gauge_telemetry::common::{CommandInvoked, Outcome, Surface};
//!
//! let telemetry = Telemetry::builder()
//!     .app("tome")
//!     .app_version(env!("CARGO_PKG_VERSION"))
//!     .endpoint("https://gauge.example/")
//!     .install_id_path(install_id_path)
//!     .app_env_var("TOME_TELEMETRY")
//!     .config_enabled(true)
//!     .runtime_enabled(true)
//!     .build()?;
//!
//! telemetry.emit(&CommandInvoked {
//!     command: "search".into(), duration_ms: 142,
//!     outcome: Outcome::Ok, surface: Surface::Cli,
//! });
//! telemetry.flush_blocking(DEFAULT_FLUSH_TIMEOUT);
//! # Ok::<(), gauge_telemetry::BuildError>(())
//! ```

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
