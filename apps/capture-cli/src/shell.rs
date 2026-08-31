//! The terminal capture gesture: type the text, press the chord.
//!
//! # Why this is a keybinding and not a command
//!
//! The obvious design is a short command — `b fix auth #client-acme`. It cannot
//! work, and the reason is worth stating precisely, because the broken version
//! looks fine in every non-interactive test.
//!
//! By the time any shell function runs, the shell has already finished with the
//! line. `#client-acme` was stripped as a comment. `>` redirected into a file.
//! `!` ran history expansion. `$VAR` was substituted, `*` was globbed, and
//! backticks *executed*. A function receives the wreckage and cannot recover
//! the original, because the original no longer exists anywhere.
//!
//! A keybinding runs earlier. It fires while the line is still sitting in the
//! editor's buffer, before the shell has parsed a byte of it, and reads that
//! buffer directly: `READLINE_LINE` in bash, `$BUFFER` in zsh. That is the only
//! place the text still exists exactly as typed.
//!
//! So the gesture is **type the text, press Ctrl-B** — not "type a command,
//! then the text". It is also the same shape as the overlay's gesture: a chord,
//! not a command.
//!
//! # The line is the receipt
//!
//! Both bindings clear the buffer only when capture succeeded. That gives a
//! confirmation that needs no message and cannot be missed:
//!
//! - the line vanishes — it is stored
//! - the line stays, with an error — it is not stored, and it is still there to
//!   try again or copy elsewhere
//!
//! Clearing first and reporting afterwards would be the same interface with the
//! failure case inverted: the text gone, and a message about it scrolling away.

use capture_store::{Error, Result};

/// Shells with a binding.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Shell {
    Bash,
    Zsh,
}

impl Shell {
    /// Parse a shell name, or explain what is on offer.
    pub fn parse(name: &str) -> Result<Shell> {
        match name {
            "bash" => Ok(Shell::Bash),
            "zsh" => Ok(Shell::Zsh),
            other => Err(Error::Usage(format!(
                "no capture binding for '{other}'.\n\
                 \n\
                 Available: bash, zsh\n\
                 \n\
                 Other shells can still capture with `capture +`, which reads a \
                 line without the shell touching it."
            ))),
        }
    }

    /// The snippet to evaluate in a shell's startup file.
    pub fn init(self) -> String {
        match self {
            Shell::Bash => BASH.to_string(),
            Shell::Zsh => ZSH.to_string(),
        }
    }
}

/// bash, via `bind -x`.
///
/// `READLINE_LINE` exists only inside a function invoked by a `bind -x`
/// binding. Called any other way it is unset, and the capture would silently
/// store an empty line — worse than the mangling it is meant to prevent, since
/// at least mangled text is visibly wrong.
const BASH: &str = r####"# capture — take the raw line before bash parses it.
# Add to ~/.bashrc:  eval "$(capture shell-init bash)"

_capture_overlay() {
  # READLINE_LINE is set only while a bind -x binding is running. Empty means
  # either an empty prompt or this function being called as a command, and
  # neither is something to store.
  [ -n "$READLINE_LINE" ] || return 0

  local err
  # stdout goes nowhere: the slug would land on top of the prompt. stderr is
  # kept, because a failure has to be visible.
  if err=$(printf '%s\n' "$READLINE_LINE" | capture + --stdin 2>&1 >/dev/null); then
    READLINE_LINE=''
    READLINE_POINT=0
  else
    # The line is deliberately left in the buffer: it was not saved, and
    # clearing it would destroy the only remaining copy.
    printf '\nbrain: %s\n' "${err:-capture failed}" >&2
  fi
}

# The key sequence is expanded into a variable first. Writing it inline as
#   bind -x '"${BRAIN_CAPTURE_KEY:-\C-b}": _capture_overlay'
# looks equivalent and is not: single quotes suppress expansion, so bash binds
# the literal characters ${BRAIN_CAPTURE_KEY:-\C-b} and Ctrl-B does nothing.
# The pty harness caught this; nothing else would have.
_brain_key="${BRAIN_CAPTURE_KEY:-\C-b}"
bind -x "\"$_brain_key\": _capture_overlay"
unset _brain_key
"####;

/// zsh, via a ZLE widget.
///
/// `$BUFFER` is the line editor's buffer, readable only from inside a widget.
/// `print -r` is required: without `-r`, `print` interprets backslash escapes,
/// so a captured Windows path or a regex would arrive altered.
const ZSH: &str = r####"# capture — take the raw line before zsh parses it.
# Add to ~/.zshrc:  eval "$(capture shell-init zsh)"

_capture_overlay() {
  [[ -n $BUFFER ]] || return 0

  local err
  # -r stops print interpreting backslashes; without it a path like
  # C:\temp\new would arrive with a tab and a newline in it.
  if err=$(print -r -- "$BUFFER" | capture + --stdin 2>&1 >/dev/null); then
    BUFFER=''
    CURSOR=0
  else
    # zle -M writes below the prompt without disturbing the line, which is
    # still holding the only copy of the text.
    zle -M "capture: ${err:-capture failed}"
  fi
}

zle -N _capture_overlay
bindkey "${BRAIN_CAPTURE_KEY:-^B}" _capture_overlay
"####;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_shells_have_a_binding() {
        assert_eq!(Shell::parse("bash").unwrap(), Shell::Bash);
        assert_eq!(Shell::parse("zsh").unwrap(), Shell::Zsh);
    }

    #[test]
    fn an_unknown_shell_is_told_about_the_fallback() {
        // Someone on fish is not stuck; they just do not get the chord.
        let e = Shell::parse("fish").unwrap_err();
        assert!(format!("{e}").contains("capture +"));
    }

    #[test]
    fn neither_binding_is_a_plain_command() {
        // The failure this whole module exists to prevent. A function invoked
        // as a command receives a line the shell has already split, expanded
        // and truncated; only a binding sees the buffer.
        for s in [Shell::Bash, Shell::Zsh] {
            let t = s.init();
            assert!(
                t.contains("bind -x") || t.contains("bindkey"),
                "the capture must be bound to a key, not defined as a command"
            );
        }
    }

    #[test]
    fn each_binding_reads_its_shells_raw_buffer() {
        assert!(Shell::Bash.init().contains("READLINE_LINE"));
        assert!(Shell::Zsh.init().contains("$BUFFER"));
    }

    #[test]
    fn zsh_prints_raw() {
        // Without -r, print expands backslash escapes and a captured path or
        // regex arrives altered — the exact class of silent corruption the
        // keybinding exists to avoid.
        assert!(Shell::Zsh.init().contains("print -r --"));
    }

    #[test]
    fn the_line_is_cleared_only_after_a_successful_capture() {
        // The buffer holds the only copy of the text. Clearing it before the
        // capture is known to have worked turns a failed write into lost data,
        // and the clearing is also the only confirmation the user gets.
        for s in [Shell::Bash, Shell::Zsh] {
            let t = s.init();
            let ok = t.find("if err=").expect("the capture is tested");
            let cleared = t.find("BUFFER=''").or_else(|| t.find("READLINE_LINE=''"));
            assert!(
                cleared.expect("the buffer is cleared somewhere") > ok,
                "clearing must be inside the success branch"
            );
        }
    }

    #[test]
    fn the_chord_can_be_changed() {
        for s in [Shell::Bash, Shell::Zsh] {
            assert!(s.init().contains("BRAIN_CAPTURE_KEY"));
        }
    }

    #[test]
    fn stdout_is_discarded_but_errors_are_not() {
        // The slug would land on top of the prompt; a failure must not be
        // swallowed with it.
        for s in [Shell::Bash, Shell::Zsh] {
            assert!(s.init().contains("2>&1 >/dev/null"));
        }
    }
}
