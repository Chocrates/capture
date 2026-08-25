//! The window: one text field, Enter commits, Escape discards.
//!
//! # Pre-warmed, never created on demand
//!
//! The window is built at startup and then hidden. Showing it is a visibility
//! change, not a construction — no process start, no font loading, no GPU
//! context. That is the whole reason the overlay is resident: the gesture has
//! to feel like it already happened, and building a window on a keypress cannot.
//!
//! # There is no button that creates a note
//!
//! Deliberately. An on-screen control becomes the path of least resistance, and
//! once it exists the gesture is no longer the same everywhere — which was the
//! point of having one gesture. The only way in is the chord.
//!
//! # Escape discards, and says nothing
//!
//! No "are you sure". A confirmation on discard would train the reflex that
//! dismisses dialogs, and that reflex is the one thing standing between the
//! user and the redaction confirmation later. Discard is cheap because capture
//! is cheap: type it again.
//!
//! # What the window does not do
//!
//! It does not parse, validate, autocomplete, suggest, or reformat. Capture is
//! verbatim, and every one of those would be a decision the user has to make at
//! the moment they are trying not to make decisions.

// Timing how long the window takes to appear. The crate-wide clock ban exists
// so the fold cannot consult a clock; this is a stopwatch on the user
// interface, and the reading never reaches an event.
#![allow(
    clippy::disallowed_methods,
    clippy::disallowed_types,
    reason = "measuring how long the overlay takes to draw, not stamping an event"
)]

use std::sync::mpsc::Receiver;

use brain_store::StorePaths;

/// How the overlay reports what happened, for the caller to act on.
pub enum Outcome {
    /// Text was committed.
    Captured(String),
    /// The window was dismissed with nothing kept.
    Discarded,
}

/// The overlay's state between frames.
pub struct Overlay {
    /// What is being typed. Never inspected, only stored.
    text: String,
    /// Whether the window is currently on screen.
    visible: bool,
    /// Requests to show, arriving from the socket thread.
    shows: Receiver<()>,
    /// Where captures go.
    paths: StorePaths,
    /// The last thing that happened, shown briefly.
    said: Option<String>,
    /// When the current show request arrived, until the first frame measures it.
    shown_at: Option<std::time::Instant>,
}

impl Overlay {
    pub fn new(paths: StorePaths, shows: Receiver<()>) -> Overlay {
        Overlay {
            text: String::new(),
            visible: false,
            shows,
            paths,
            said: None,
            shown_at: None,
        }
    }

    /// Whether the window should be on screen right now.
    pub fn visible(&self) -> bool {
        self.visible
    }

    /// Take any pending show requests.
    ///
    /// Drains rather than handling one: several presses while hidden mean the
    /// same thing as one, and queueing them would show the window again each
    /// time it was dismissed.
    pub fn poll(&mut self) -> bool {
        let mut asked = false;
        while self.shows.try_recv().is_ok() {
            asked = true;
        }
        if asked {
            self.visible = true;
            self.said = None;
            // Started here, at the moment the request arrived, and stopped by
            // the first frame that draws. Anything measured from later than
            // this would leave out the part most likely to be slow.
            self.shown_at = Some(std::time::Instant::now());
        }
        asked
    }

    /// Commit what is typed.
    ///
    /// Empty input is discarded rather than stored — a note with no content
    /// cannot be found again, and pressing Enter on an empty field is far more
    /// likely to be a dismissal than an intention.
    pub fn commit(&mut self) -> Option<Outcome> {
        let text = std::mem::take(&mut self.text);
        self.visible = false;
        if text.trim().is_empty() {
            return Some(Outcome::Discarded);
        }
        Some(Outcome::Captured(text))
    }

    /// Throw away what is typed.
    pub fn discard(&mut self) -> Outcome {
        self.text.clear();
        self.visible = false;
        Outcome::Discarded
    }

    /// Store a capture, and say what became of it.
    ///
    /// The window closes before this runs. Waiting for the store would make the
    /// gesture's duration depend on the disk, and the whole design of the
    /// capture path — spool included — exists so that it never has to.
    pub fn store(&mut self, text: &str) {
        let said = match brain_store::session::capture(&self.paths, text) {
            Ok(brain_store::session::Committed::Stored(slug)) => slug,
            // Safe, and not findable yet. Saying so is the point: the note is
            // not lost, but it will not turn up in a search until the store
            // takes it.
            Ok(brain_store::session::Committed::Spooled) => {
                "spooled — the store could not be written".to_string()
            }
            Err(e) => format!("not saved: {e}"),
        };
        self.said = Some(said);
    }

    /// Note that a frame has been drawn, and time how long it took.
    ///
    /// Called from the drawing pass. The measurement is taken once per
    /// appearance — every later frame in the same appearance is a repaint, and
    /// counting those would fill the histogram with numbers describing nothing.
    pub fn drawn(&mut self) {
        let Some(started) = self.shown_at.take() else {
            return;
        };
        let ms = started.elapsed().as_millis() as u64;
        if let Ok(ops) = brain_store::ops::Ops::open(&self.paths.ops) {
            let _ = ops.record_latency(ms);
        }
        tracing::debug!(ms, "overlay shown");
    }

    /// The current text, for the view to render.
    pub fn text_mut(&mut self) -> &mut String {
        &mut self.text
    }

    /// The last thing that happened.
    pub fn said(&self) -> Option<&str> {
        self.said.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    fn overlay() -> (Overlay, mpsc::Sender<()>) {
        let (tx, rx) = mpsc::channel();
        let d = std::env::temp_dir().join(format!("brain-ov-{}", std::process::id()));
        (Overlay::new(StorePaths::under(d), rx), tx)
    }

    #[test]
    fn it_starts_hidden() {
        // Resident and invisible. A window that appears at login is a window
        // that gets closed at login.
        let (o, _tx) = overlay();
        assert!(!o.visible());
    }

    #[test]
    fn a_request_shows_it() {
        let (mut o, tx) = overlay();
        tx.send(()).unwrap();
        assert!(o.poll());
        assert!(o.visible());
    }

    #[test]
    fn repeated_presses_while_hidden_are_one_request() {
        // Otherwise the window reappears once per press after being dismissed,
        // and an impatient double-press becomes two windows to close.
        let (mut o, tx) = overlay();
        tx.send(()).unwrap();
        tx.send(()).unwrap();
        tx.send(()).unwrap();
        assert!(o.poll());
        o.discard();
        assert!(!o.poll(), "the queue is empty, not three deep");
        assert!(!o.visible());
    }

    #[test]
    fn committing_text_returns_it_and_closes() {
        let (mut o, tx) = overlay();
        tx.send(()).unwrap();
        o.poll();
        o.text_mut().push_str("buy milk");

        match o.commit() {
            Some(Outcome::Captured(t)) => assert_eq!(t, "buy milk"),
            _ => panic!("expected the text back"),
        }
        assert!(!o.visible());
    }

    #[test]
    fn the_field_is_empty_next_time() {
        // Leftover text is worse than it sounds: the next capture starts with
        // somebody else's sentence already in it, and Enter is one keystroke.
        let (mut o, tx) = overlay();
        tx.send(()).unwrap();
        o.poll();
        o.text_mut().push_str("first thing");
        o.commit();

        tx.send(()).unwrap();
        o.poll();
        assert!(o.text_mut().is_empty());
    }

    #[test]
    fn committing_nothing_stores_nothing() {
        // Enter on an empty field is a dismissal far more often than an
        // intention, and a note with no content cannot be found again.
        let (mut o, tx) = overlay();
        tx.send(()).unwrap();
        o.poll();
        assert!(matches!(o.commit(), Some(Outcome::Discarded)));
    }

    #[test]
    fn whitespace_alone_stores_nothing() {
        let (mut o, tx) = overlay();
        tx.send(()).unwrap();
        o.poll();
        o.text_mut().push_str("   \n\t ");
        assert!(matches!(o.commit(), Some(Outcome::Discarded)));
    }

    #[test]
    fn discarding_keeps_nothing_and_asks_nothing() {
        // No confirmation, on purpose. A prompt here would train the reflex
        // that dismisses dialogs — the same reflex the redaction confirmation
        // has to survive.
        let (mut o, tx) = overlay();
        tx.send(()).unwrap();
        o.poll();
        o.text_mut().push_str("never mind");

        assert!(matches!(o.discard(), Outcome::Discarded));
        assert!(!o.visible());
        assert!(o.text_mut().is_empty());
    }

    #[test]
    fn text_is_kept_exactly_as_typed() {
        // The window parses nothing. Every sigil this system recognises is
        // stored as a character, and interpretation happens later or not at all.
        let (mut o, tx) = overlay();
        tx.send(()).unwrap();
        o.poll();
        let odd = "#tag >file $VAR `cmd` *glob ~home !bang";
        o.text_mut().push_str(odd);
        match o.commit() {
            Some(Outcome::Captured(t)) => assert_eq!(t, odd),
            _ => panic!("expected the text back"),
        }
    }
}
