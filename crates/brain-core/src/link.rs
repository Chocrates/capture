//! Links written inside note text.
//!
//! # A link records what it meant, not where it points now
//!
//! A link is written as `[[some-name]]`. What gets stored is **both** the name
//! as written and the note that name resolved to at the moment of writing.
//!
//! Storing only the name would make every link a lookup against the present. If
//! a name later moves to a different note — an alias reassigned, a slug freed —
//! every old reference silently starts pointing at content its author never
//! saw. In a system whose whole job is to be trustworthy about what you
//! recorded, quietly repointing an old reference at different material is close
//! to the worst thing it could do.
//!
//! Storing only the target would lose the reverse case: the name is what the
//! author actually typed, and a link that can no longer show it cannot explain
//! why it is broken.
//!
//! So both are kept, and when they disagree the link renders as **broken**,
//! showing both names. Broken is a fine outcome. Wrong is not.
//!
//! # Resolution happens during the fold, not on read
//!
//! The fold walks events in timestamp order, so at the moment a link is read
//! the set of notes that existed *then* is exactly what has been folded so far.
//! Resolving there gives the authoring-time answer for free, and gives it
//! identically on every replica.
//!
//! A link to a name that did not exist yet resolves to nothing. That is not an
//! error: forward references are a normal way to write, and the link simply has
//! no authoring-time target to be compared against later.

use alloc::string::String;
use alloc::vec::Vec;

use minicbor::{Decode, Encode};

use crate::id::EventId;

/// A link found in note text.
#[derive(Clone, PartialEq, Eq, Debug, Encode, Decode)]
pub struct Link {
    /// Exactly the text between the brackets, untouched.
    #[n(0)]
    pub name: String,
    /// What that name meant when the link was written. `None` if the name
    /// matched nothing at the time.
    #[n(1)]
    pub resolved: Option<EventId>,
}

/// The longest a link name may be.
///
/// Bounded so a malformed note cannot produce an enormous row. Two brackets and
/// a runaway paragraph is otherwise a valid-looking link.
pub const NAME_CAP: usize = 200;

/// Pull `[[name]]` links out of text.
///
/// Deliberately simple: no nesting, no escapes, no markdown. Captured text is
/// stored verbatim and is not a document format, so anything cleverer would be
/// inventing syntax the user never agreed to — and every rule added here is a
/// rule that can surprise someone who typed two brackets for another reason.
///
/// Duplicates are preserved in order. A note that links the same place twice
/// really does link it twice, and collapsing them would misreport the text.
pub fn extract(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0;

    while i + 3 < bytes.len() {
        if bytes[i] != b'[' || bytes[i + 1] != b'[' {
            i += 1;
            continue;
        }
        let start = i + 2;
        let Some(end) = find_close(bytes, start) else {
            // No closing brackets anywhere after this. Nothing later can be a
            // link either, so stop rather than rescanning the tail.
            break;
        };
        let name = &text[start..end];
        if !name.is_empty() && name.len() <= NAME_CAP && !name.contains('\n') {
            out.push(String::from(name));
        }
        i = end + 2;
    }
    out
}

fn find_close(bytes: &[u8], from: usize) -> Option<usize> {
    let mut i = from;
    while i + 1 < bytes.len() {
        if bytes[i] == b']' && bytes[i + 1] == b']' {
            return Some(i);
        }
        i += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString;

    fn names(t: &str) -> Vec<String> {
        extract(t)
    }

    #[test]
    fn a_plain_link_is_found() {
        assert_eq!(names("see [[budget-notes]] for detail"), ["budget-notes"]);
    }

    #[test]
    fn several_links_are_found_in_order() {
        assert_eq!(
            names("[[first]] then [[second]] then [[third]]"),
            ["first", "second", "third"]
        );
    }

    #[test]
    fn a_repeated_link_is_kept_twice() {
        // The note really does link it twice; collapsing would misreport what
        // was written.
        assert_eq!(names("[[a]] and again [[a]]"), ["a", "a"]);
    }

    #[test]
    fn text_without_links_yields_none() {
        assert!(names("nothing to see here").is_empty());
        assert!(names("").is_empty());
    }

    #[test]
    fn a_single_bracket_is_not_a_link() {
        assert!(names("array[0] and [x] alone").is_empty());
    }

    #[test]
    fn an_unclosed_link_is_not_a_link() {
        // Someone mid-sentence, or someone who typed brackets for another
        // reason. Guessing where it ends would invent content.
        assert!(names("this [[never closes").is_empty());
    }

    #[test]
    fn an_empty_link_is_ignored() {
        assert!(names("[[]] is nothing").is_empty());
    }

    #[test]
    fn a_link_may_not_span_lines() {
        // Two brackets at the end of one line and two at the start of another
        // are almost certainly not a link, and treating them as one would
        // swallow a whole line of text into a name.
        assert!(names("[[start\nend]]").is_empty());
    }

    #[test]
    fn a_runaway_name_is_ignored() {
        let long = "x".repeat(NAME_CAP + 1);
        assert!(names(&alloc::format!("[[{long}]]")).is_empty());
    }

    #[test]
    fn the_name_is_taken_exactly_as_written() {
        // No trimming, no case folding, no normalising. Resolution may be
        // stricter later, but what the author typed is what is recorded.
        assert_eq!(names("[[ Spaced Name ]]"), [" Spaced Name "]);
    }

    #[test]
    fn unicode_names_survive() {
        assert_eq!(names("[[café-notes]]"), ["café-notes"]);
        assert_eq!(names("[[日本語]]"), ["日本語"]);
    }

    #[test]
    fn scanning_stops_after_the_last_close() {
        // A pathological line of open brackets must not become quadratic.
        let t = "[[".repeat(5000);
        assert!(names(&t).is_empty());
    }

    #[test]
    fn a_link_inside_other_punctuation_is_still_found() {
        assert_eq!(names("(see [[a-note]], later)"), ["a-note"]);
    }

    #[test]
    fn a_link_record_carries_both_halves() {
        // The point of the type: a broken link can still show what was meant.
        let l = Link {
            name: "old-name".to_string(),
            resolved: Some(EventId([7u8; 32])),
        };
        assert_eq!(l.name, "old-name");
        assert!(l.resolved.is_some());
    }
}
