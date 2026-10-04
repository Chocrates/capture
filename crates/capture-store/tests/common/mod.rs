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
