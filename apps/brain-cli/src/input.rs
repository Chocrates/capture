//! Getting the text of a capture.
//!
//! # Refusing to store nothing
//!
//! An empty capture is refused everywhere: no arguments, an empty pipe, an
//! empty line at the prompt. Storing it would create a note with no content and
//! a slug derived from no words, which then appears in the timeline as a blank
//! row that cannot be searched for and is hard to identify well enough to
//! delete.
//!
//! Whitespace counts as empty. A stray newline through a pipe is the most
//! likely way to produce one of these, and it is never deliberate.

use std::io::{IsTerminal, Read, Write};

use brain_store::{Error, Result};

use crate::args::Source;

/// Read the text a capture is to store.
pub fn read(source: &Source) -> Result<String> {
    let raw = match source {
        Source::Words(w) => w.clone(),
        Source::Stdin => slurp()?,
        Source::Unspecified => {
            // The only decision made from the environment rather than the
            // request. A pipe with nothing attached to it would otherwise block
            // forever waiting for a person who is not there, and a terminal
            // would otherwise read end-of-file immediately and refuse.
            if std::io::stdin().is_terminal() {
                prompt()?
            } else {
                slurp()?
            }
        }
    };

    let text = raw.trim();
    if text.is_empty() {
        return Err(Error::Usage(
            "nothing to capture — a note with no content cannot be found again".into(),
        ));
    }
    Ok(text.to_string())
}

fn slurp() -> Result<String> {
    let mut s = String::new();
    std::io::stdin().read_to_string(&mut s)?;
    Ok(s)
}

/// Ask for a line, at a terminal.
///
/// The terminal is left in its ordinary mode, so the line editing available is
/// whatever the terminal driver provides: backspace, word erase, kill line.
/// That is genuinely less than a shell offers — no history, no cursor movement
/// — and it is deliberate for now, because the alternative is putting the
/// terminal into raw mode and writing an editor, and the gesture this tool is
/// actually built around does not come through here at all.
///
/// The intended path is the keybinding: type the text into the shell prompt you
/// are already at, press the chord, and the raw line is sent before the shell
/// touches it. This prompt is the fallback for someone who typed `brain +` and
/// pressed return.
fn prompt() -> Result<String> {
    let mut out = std::io::stdout();
    write!(out, "capture: ")?;
    out.flush()?;

    let mut line = String::new();
    std::io::stdin().read_line(&mut line)?;
    Ok(line)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_are_taken_as_given() {
        assert_eq!(read(&Source::Words("buy milk".into())).unwrap(), "buy milk");
    }

    #[test]
    fn surrounding_whitespace_is_removed() {
        assert_eq!(read(&Source::Words("  spaced  ".into())).unwrap(), "spaced");
    }

    #[test]
    fn interior_text_is_never_touched() {
        // Capture is verbatim. A misparse must be a wrong label on an intact
        // note, never a rewritten one, so nothing here normalises content.
        let odd = "line one\n  indented\ttab  #tag $VAR `tick`";
        assert_eq!(read(&Source::Words(odd.into())).unwrap(), odd);
    }

    #[test]
    fn an_empty_capture_is_refused() {
        assert!(read(&Source::Words(String::new())).is_err());
    }

    #[test]
    fn whitespace_alone_is_refused() {
        // The likely accident: a stray newline arriving down a pipe.
        assert!(read(&Source::Words("   \n\t ".into())).is_err());
    }

    #[test]
    fn refusing_an_empty_capture_is_a_usage_error() {
        // Not "not found" and certainly not an integrity failure: the request
        // was wrong, and a script checking exit codes needs to see that.
        let e = read(&Source::Words(String::new())).unwrap_err();
        assert_eq!(e.exit_code(), brain_store::ExitCode::Usage);
    }
}
