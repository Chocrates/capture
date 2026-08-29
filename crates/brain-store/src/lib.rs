//! `brain-store` — SQLite, entropy, clocks, and the filesystem.
//!
//! This crate exists so that [`brain_core`] does not have to. Everything impure
//! that the core logically needs is produced here and passed **in as a value**:
//! the body nonce, the incarnation, the HLC stamp, the capture timezone.
//!
//! The temptation is to hand the core a `Clock` trait object instead. That is
//! the same mistake wearing an interface — once it exists, nothing stops the
//! fold taking it, and the fold must be a function of the event set alone.

pub mod ambient;
pub mod db;
pub mod descriptor;
pub mod drain;
pub mod due;
pub mod entropy;
pub mod error;
pub mod gate;
pub mod genesis;
pub mod gesture;
pub mod hotkey;
pub mod identity;
pub mod incremental;
pub mod latency;
pub mod lock;
pub mod log;
pub mod mutate;
pub mod notifier;
pub mod ops;
pub mod paths;
pub mod projection;
pub mod promote;
pub mod purge;
pub mod ratelimit;
pub mod review;
pub mod session;
pub mod settings;
pub mod spool;
pub mod startup;
pub mod store;
pub mod watch;

pub use error::{Error, ExitCode, Result};
pub use identity::Identity;
pub use paths::StorePaths;
pub use store::FreshStore;
