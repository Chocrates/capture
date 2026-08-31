//! BSP — the Brain Sync Protocol (DESIGN.md §5).
//!
//! # This crate owns the wire and nothing else
//!
//! It hands `brain-store` verified envelopes. It does not fold them, does not
//! decide what any of them means, and never touches the projection. The reason
//! is the same one that keeps a clock out of `brain-core`: a socket is the
//! least deterministic thing in the system, and the boundary between "events
//! arrived" and "events were folded" is exactly where a bug stops being
//! reproducible.
//!
//! Everything here is written against [`Transport`], and the tests use an
//! in-memory one. A suite that needs the network is a suite that gets skipped,
//! and this is the milestone where a skipped test costs the most.
//!
//! # What the protocol is, in one paragraph
//!
//! Events are single-writer-per-origin with a gap-free `seq`, so a replica's
//! whole state is one integer per origin. Two peers exchange those in `HELLO`,
//! each computes its own difference without a negotiation round, and asks for
//! what it lacks with `RANGE`. There is no Merkle tree, no bloom filter and no
//! set-difference estimator, because the complexity that normally lives in a
//! sync engine was paid for in the data model in M1.
//!
//! Correctness lives in anti-entropy. Live push (§5.4) is a latency
//! optimisation that is allowed to fail silently — with one exception, and it
//! is an exception in urgency rather than in correctness: a `redact` is pushed
//! eagerly and retried until acknowledged, because for a redaction the delivery
//! window is exactly the window in which the content can still be read.

#![forbid(unsafe_code)]

// Not `no_std`, and it never could have been: this crate owns TCP and a Noise
// implementation, both of which need an operating system. The purity that
// matters here is a different one — that nothing in this crate folds an event
// or touches the projection — and that is enforced by what it depends on
// rather than by a lint.
extern crate alloc;

pub mod anti_entropy;
pub mod discovery;
#[cfg(feature = "mdns")]
pub mod mdns;
pub mod message;
pub mod pairing;
pub mod session;
pub mod tcp;
pub mod transport;

pub use message::{Message, PROTOCOL_VERSION, VersionVector};
pub use session::{HandshakeError, Session};
pub use transport::{Transport, TransportError};
