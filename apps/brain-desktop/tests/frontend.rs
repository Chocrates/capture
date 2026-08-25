//! Checks on the parts of the interface that are not Rust.
//!
//! # Why a text check rather than a browser
//!
//! Driving a real webview from a test needs a display, a driver and a protocol,
//! and it would still be checking the same handful of facts. These are cheap,
//! they run everywhere, and each one exists because of a specific failure.
//!
//! They are deliberately narrow. A test that tried to assert what the page
//! *looks like* would be re-implementing a browser badly; these assert only
//! things with a one-line answer and a real consequence.

use std::path::PathBuf;

fn ui(file: &str) -> String {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("ui")
        .join(file);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("reading {}: {e}", p.display()))
}

/// The file with its line comments removed.
///
/// Checks for a forbidden construct have to ignore comments, or the comment
/// *explaining* why the construct is forbidden trips the check that enforces
/// it — and the obvious way to make the test pass is then to delete the
/// explanation.
fn ui_code(file: &str) -> String {
    ui(file)
        .lines()
        .map(|l| match l.find("//") {
            Some(i) if !l[..i].contains('"') => &l[..i],
            _ => l,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn hidden_beats_every_author_style() {
    // The bug this exists for: `#confirm { display: grid }` is an author-level
    // declaration, and the `hidden` attribute only works through the user-agent
    // stylesheet's `[hidden] { display: none }` — which author styles beat. The
    // redaction dialog was therefore visible from the moment the page loaded.
    //
    // The destructive confirmation appearing unprompted, on startup, before the
    // user has done anything, is the fastest possible way to train the reflex
    // it is built to survive.
    let css = ui("style.css");
    assert!(
        css.contains("[hidden] { display: none !important; }"),
        "the stylesheet must force `hidden` to win, or any element given a \
         `display` becomes permanently visible"
    );
}

#[test]
fn everything_marked_hidden_stays_hidden_until_asked_for() {
    // Every element the markup hides is one the interface reveals deliberately.
    // If a rule below gave one of them a `display`, it would be on screen from
    // the start — and the check above is what stops that.
    let html = ui("index.html");
    for id in ["confirm", "toast"] {
        assert!(
            html.contains(&format!("id=\"{id}\"")) && html.contains("hidden"),
            "{id} should start hidden"
        );
    }
}

#[test]
fn the_destroy_button_starts_disabled() {
    // A default-focused accept is what makes a dialog dismissible by reflex.
    // This one has to be typed into before it will do anything.
    let html = ui("index.html");
    assert!(
        html.contains("id=\"confirm-go\" disabled"),
        "the accept button must be disabled until the note's name is typed"
    );
}

#[test]
fn note_text_is_never_treated_as_markup() {
    // Captures are stored verbatim and are expected to contain anything at all,
    // including half-written HTML. Rendering any of it as markup would let a
    // note change the page displaying it.
    let js = ui_code("app.js");
    assert!(
        !js.contains("innerHTML"),
        "note content must go in through textContent"
    );
}

#[test]
fn nothing_on_screen_creates_a_note() {
    // An on-screen control becomes the path of least resistance, and once it
    // exists the capture gesture is no longer the same everywhere — which was
    // the entire point of having one gesture.
    let html = ui("index.html").to_lowercase();
    for word in ["new note", "add note", "create note", "+ note"] {
        assert!(
            !html.contains(word),
            "found a note-creating control: {word}"
        );
    }
}

#[test]
fn the_page_talks_only_to_the_commands_it_is_given() {
    // No fetch, no URL, no server. The capability list is the invoke handler in
    // main.rs, and it is only a real boundary if the page has no other way out.
    let js = ui_code("app.js");
    for escape in ["fetch(", "XMLHttpRequest", "WebSocket", "import("] {
        assert!(!js.contains(escape), "the page reached for {escape}");
    }
}

#[test]
fn every_command_the_page_calls_is_registered() {
    // A typo in a command name is invisible until the button is pressed, and
    // then it fails at the worst moment — halfway through something the user
    // asked for.
    let js = ui("app.js");
    let main =
        std::fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/main.rs"))
            .unwrap();

    let mut called: Vec<String> = Vec::new();
    for pattern in ["invoke(\"", "call(\""] {
        let mut rest = js.as_str();
        while let Some(i) = rest.find(pattern) {
            rest = &rest[i + pattern.len()..];
            if let Some(end) = rest.find('"') {
                called.push(rest[..end].to_string());
            }
        }
    }
    assert!(!called.is_empty(), "the page calls something");

    for name in called {
        assert!(
            main.contains(&format!("ipc::{name},")),
            "the page calls `{name}`, which is not in the invoke handler"
        );
    }
}
