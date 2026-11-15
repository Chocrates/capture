//! Writing to standard output without falling over when the reader leaves.
//!
//! # Why `println!` is not used anywhere in this binary
//!
//! `capture show something | head -3` closes the pipe as soon as `head` has what
//! it wants. The next write fails with `EPIPE`, and because Rust ignores
//! `SIGPIPE` by default, `println!` responds by **panicking**:
//!
//! ```text
//! thread 'main' panicked at library/std/src/io/stdio.rs:
//! failed printing to stdout: Broken pipe (os error 32)
//! ```
//!
//! A backtrace, on a command that did exactly what was asked of it. Piping into
//! `head`, `less` or `grep -q` is ordinary use, not an error, and the ordinary
//! behaviour for a unix tool is to stop quietly.
//!
//! So every line of output goes through here, and a closed pipe ends the
//! process with success. The alternative — restoring the default `SIGPIPE`
//! handler — gets the same result by having the kernel kill the process, but it
//! needs a raw signal call and it kills it *everywhere*, including in the middle
//! of a write to the store.

/// Print a line, or stop if nobody is reading any more.
#[macro_export]
macro_rules! say {
    () => { $crate::out::line("") };
    ($($arg:tt)*) => { $crate::out::line(&format!($($arg)*)) };
}

/// Print without a newline, same rules.
#[macro_export]
macro_rules! emit {
    ($($arg:tt)*) => { $crate::out::raw(&format!($($arg)*)) };
}

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

/// Set when a write command was asked for `--json`.
///
/// A write's prose then goes to stderr, where a person running it still reads
/// it, and stdout carries the one object a program parses
/// (an-agent-keeps-notes-through-the-cli D4). Done here rather than in each of
/// forty write commands so that none of them can be the one that forgot.
static WRITE_JSON: AtomicBool = AtomicBool::new(false);

/// What a write made, when the command did not already name it.
static WROTE: Mutex<Option<String>> = Mutex::new(None);

/// Something the write wants said beside its answer (a name that was taken).
static NOTICE: Mutex<Option<String>> = Mutex::new(None);

/// Send prose to stderr and keep stdout for one JSON answer.
pub fn write_json() {
    WRITE_JSON.store(true, Ordering::Relaxed);
}

/// Whether [`write_json`] is in force.
pub fn writing_json() -> bool {
    WRITE_JSON.load(Ordering::Relaxed)
}

/// Record the name of what a write created.
pub fn wrote(name: &str) {
    *WROTE.lock().unwrap_or_else(|e| e.into_inner()) = Some(name.to_string());
}

/// The name [`wrote`] recorded.
pub fn written() -> Option<String> {
    WROTE.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// Record a notice for the JSON answer. Prose prints its own.
pub fn notice(text: &str) {
    *NOTICE.lock().unwrap_or_else(|e| e.into_inner()) = Some(text.to_string());
}

/// The notice [`notice`] recorded.
pub fn noticed() -> Option<String> {
    NOTICE.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// Write one line, exiting quietly if the pipe is closed.
pub fn line(s: &str) {
    use std::io::Write;
    if writing_json() {
        let _ = writeln!(std::io::stderr().lock(), "{s}");
        return;
    }
    let mut out = std::io::stdout().lock();
    if writeln!(out, "{s}").is_err() {
        finish();
    }
}

/// Write the one JSON answer a write gives, always to stdout.
pub fn answer_line(s: &str) {
    use std::io::Write;
    let mut out = std::io::stdout().lock();
    if writeln!(out, "{s}").is_err() {
        finish();
    }
}

/// Write without a trailing newline.
pub fn raw(s: &str) {
    use std::io::Write;
    if writing_json() {
        let _ = write!(std::io::stderr().lock(), "{s}");
        return;
    }
    let mut out = std::io::stdout().lock();
    if write!(out, "{s}").is_err() {
        finish();
    }
}

/// The reader has gone. Nothing has failed, so this is a success.
///
/// Exiting rather than returning an error: every caller would otherwise have to
/// thread a "the pipe closed" case through code whose only sensible response is
/// to stop, and the first one to forget would bring the panic back.
fn finish() -> ! {
    std::process::exit(0)
}
