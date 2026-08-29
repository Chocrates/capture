//! The recognizer is handed whatever somebody typed.
//!
//! Captures are stored verbatim and are expected to contain anything at all —
//! an em dash, an ellipsis, an emoji, a right-to-left name, half a paste. The
//! parser runs inside the projection write, which runs inside a commit, so a
//! panic here is not a bad reading: it is the writer dying after the event was
//! made durable, leaving the projection behind the log.
//!
//! That happened. `ordinal` split a word two bytes from its end to look for
//! "st"/"nd"/"rd"/"th", and `split_at` takes a byte index — so every word
//! ending in a character wider than ASCII cut one in half and panicked. It was
//! found by writing an ordinary sentence with a dash in it.

use proptest::prelude::*;

fn read(s: &str) {
    // Every entry point that walks the text, because they share the tokenizer
    // and any of them could be the one a surface calls.
    let _ = brain_parse::tokenize(s);
    let _ = brain_parse::mentions(s);
    let zone = brain_time::Zone::new("UTC").unwrap();
    let _ = brain_parse::recognize(s, 1_780_326_000_000, zone);
}

#[test]
fn the_characters_that_broke_it() {
    // Pinned rather than left to the generator: these are the ones that
    // actually arrive in prose, and a regression in any of them should name
    // itself rather than showing up as a shrunk random string.
    for s in [
        "—",
        "test — dash",
        "…",
        "an ellipsis…",
        "done ✅",
        "→",
        "a → b",
        "naïve",
        "café",
        "Ω",
        "🙂",
        "note 🙂",
        "«quoted»",
        "—#tag",
        "@sarah—",
        "1st",
        "2nd",
        "14th",
        "q1",
        "1-14",
        "",
        " ",
        "\n",
        "\t",
    ] {
        read(s);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(2000))]

    #[test]
    fn no_text_at_all_can_make_the_parser_panic(s in ".*") {
        read(&s);
    }

    /// The same, weighted toward the shapes a capture actually has: words,
    /// punctuation and sigils mixed together rather than uniform noise.
    #[test]
    fn nor_can_anything_shaped_like_a_capture(
        s in prop::collection::vec(
            prop_oneof![
                "[a-z]{1,8}",
                "[0-9]{1,4}",
                Just("—".to_string()),
                Just("…".to_string()),
                Just("🙂".to_string()),
                "[#@>!~^][a-z0-9]{0,6}",
                "[.,;:'\"()-]",
            ],
            0..12,
        ).prop_map(|v| v.join(" "))
    ) {
        read(&s);
    }
}
