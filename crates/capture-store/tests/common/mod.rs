//! Helpers shared between integration suites.
//!
//! A subdirectory rather than a file beside them, because Cargo compiles every
//! top-level file in `tests/` as its own binary and `common/mod.rs` is the
//! convention for something that is not one.
//!
//! Kept small on purpose. Each suite's fixtures differ in ways that are the
//! point of that suite — what it captures, how it redacts, which clock it
//! drains at — and pulling those together would hide the differences rather
//! than share anything. What belongs here is a question with one answer.

// Each test binary that declares `mod common;` compiles the whole module, and
// uses the part it needs — so anything another suite uses is dead code here.
// That is inherent to the shared-module pattern rather than a sign of an unused
// helper: `tools/unwired.py` is what watches for those, and it reads the
// workspace rather than one binary at a time.
#![allow(dead_code, reason = "each test binary uses a different part of this")]

use capture_store::StorePaths;

/// Whether an event's bytes are still in the log.
///
/// The question several redaction suites need, and not the same as whether a
/// note is readable: a redaction hides its target from every view on ingest,
/// immediately, and only the destruction is delayed (§1.1). So a note whose
/// destruction was held is already invisible and its body is still there.
///
/// One home because it is one decision about where the bytes live. It was
/// written out twice and `make duplicated` was right about it.
pub fn body_survives(paths: &StorePaths, tgt: &capture_core::EventId) -> bool {
    capture_store::log::Log::open_read(&paths.events)
        .unwrap()
        .by_id(tgt)
        .unwrap()
        .is_some_and(|e| e.body.is_some())
}

/// The note a slug names.
///
/// Resolved *before* a redaction where a test needs it afterwards: a destroyed
/// note's slug is re-derived from its id, so the name it had stops finding it.
/// That is §1.1 limit 5 working, and it makes this the wrong thing to call
/// after a purge.
pub fn tgt_of(paths: &StorePaths, slug: &str) -> capture_core::EventId {
    capture_store::projection::current(paths)
        .unwrap()
        .resolve(slug)
        .unwrap()
        .expect("the note is named")
}

/// The real now, in UTC milliseconds, for suites whose fixtures expand or
/// review around it.
#[allow(
    clippy::disallowed_methods,
    clippy::disallowed_types,
    reason = "a test asked as of the real now"
)]
pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}
