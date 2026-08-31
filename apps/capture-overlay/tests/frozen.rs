//! The capture gesture is frozen for the duration of M2.
//!
//! §8.10 asks for this explicitly, and the reason is not tidiness. Context
//! stability is among the strongest accelerators of habit formation, and this
//! build changes surfaces repeatedly across exactly the 66–154 day window where
//! that matters most. The mitigation is to hold the one gesture still and
//! change everything around it.
//!
//! A frozen thing needs something that notices when it moves. These are text
//! checks rather than behavioural ones on purpose: the chord and the two keys
//! are constants, and a test that pressed them would be testing egui rather
//! than the decision.

use std::path::PathBuf;

fn source(file: &str) -> String {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join(file);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("reading {}: {e}", p.display()))
}

#[test]
fn the_chord_is_still_control_super_space() {
    // Named in exactly one place so the interface and the desktop cannot drift
    // apart. If this changes, somebody's muscle memory changes with it.
    assert_eq!(
        capture_store::hotkey::DEFAULT_CHORD.human(),
        "Ctrl-Super-Space"
    );
    assert_eq!(
        capture_store::hotkey::DEFAULT_CHORD.gnome(),
        "<Control><Super>space"
    );
}

#[test]
fn enter_commits_and_escape_discards() {
    // The two decisions the overlay understands, and it must stay two. Every
    // additional one is a decision at the moment the user is trying not to make
    // any (§3.1.1).
    let window = source("window.rs");
    assert!(
        window.contains("egui::Key::Enter"),
        "Enter no longer commits"
    );
    assert!(
        window.contains("egui::Key::Escape"),
        "Escape no longer discards"
    );
}

#[test]
fn nothing_in_the_overlay_parses_what_was_typed() {
    // Capture stays verbatim and the window stays out of it. §3.1 puts parsing
    // after the commit precisely so the gesture never waits, and the overlay
    // reaching for the recognizer is how that would quietly stop being true.
    for file in ["window.rs", "overlay.rs"] {
        assert!(
            !source(file).contains("recognize("),
            "{file} is parsing captures; §3.1 puts that after the commit"
        );
    }
}

#[test]
fn the_overlay_shows_the_language_without_acting_on_it() {
    // The rows under the field come from capture-parse's tables, which is display
    // rather than parsing — and worth distinguishing, because it is the one
    // legitimate reason the overlay names that crate at all.
    let window = source("window.rs");
    assert!(window.contains("capture_parse::SIGILS"));
    assert!(window.contains("capture_parse::SHAPES"));
}

#[test]
fn a_capture_still_opens_the_session_that_reads_nothing() {
    // The path a capture takes must not catch the store up. A sweep can touch
    // two hundred notes, and §3.1 will not spend that on the gesture.
    let session = std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../crates/capture-store/src/session.rs"),
    )
    .expect("reading session.rs");

    assert!(
        session.contains("Session::open_for_capture(paths.clone())"),
        "the capture path no longer opts out of the startup sweep"
    );
}
