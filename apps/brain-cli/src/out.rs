//! Writing to standard output without falling over when the reader leaves.
//!
//! # Why `println!` is not used anywhere in this binary
//!
//! `brain show something | head -3` closes the pipe as soon as `head` has what
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

/// Write one line, exiting quietly if the pipe is closed.
pub fn line(s: &str) {
    use std::io::Write;
    let mut out = std::io::stdout().lock();
    if writeln!(out, "{s}").is_err() {
        finish();
    }
}

/// Write without a trailing newline.
pub fn raw(s: &str) {
    use std::io::Write;
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
