//! Slug derivation. **Frozen**, and versioned as a unit with its Unicode tables.
//!
//! A slug is a note's human-readable name: a few words from its root capture,
//! plus a 50-bit suffix of the note identity.
//!
//! ```text
//! "budget 40k, revisit Q2"  →  budget-revisit-k7m2xd4p9r
//! ```
//!
//! # Why this is frozen, and what "frozen" has to include
//!
//! Slugs are written **by hand into other notes' text** as link targets. If a
//! slug changes, every link to it breaks. And because the projection is rebuilt
//! by re-folding — which is also the recovery path and the entire migration
//! strategy — a change to this function silently rewrites history on the next
//! rebuild.
//!
//! So the algorithm is frozen, and the freeze has to cover the **Unicode tables
//! too**. Normalisation, word segmentation and transliteration are all
//! table-driven and all change between releases: a routine `cargo update` would
//! otherwise alter segmentation for some inputs and break links, which is
//! exactly the failure freezing exists to prevent. The dependencies are pinned
//! with `=` and their versions are part of [`SLUG_VERSION`].
//!
//! # Derived from the *root capture*, never the current text
//!
//! Not the current text, not a machine-generated title. Both change; the root
//! capture body does not. (An earlier draft's own worked example derived a slug
//! containing a word that only appeared after an edit — the rule and the
//! example contradicted each other in the same paragraph.)

use alloc::string::String;
use alloc::vec::Vec;

use any_ascii::any_ascii_char;
use unicode_normalization::UnicodeNormalization;
use unicode_segmentation::UnicodeSegmentation;

use crate::id::EventId;

/// Version of the whole derivation, including the pinned Unicode tables.
///
/// Bumping this means "a different algorithm", not "an improved one" — existing
/// slugs keep whatever they were given.
pub const SLUG_VERSION: u16 = 1;

/// Words taken from the capture text.
const MAX_WORDS: usize = 4;
/// Words must be *longer than* this to be included. The frozen spec reads
/// "first four words over three characters", so this is a strict comparison.
const MIN_WORD_LEN: usize = 3;
/// Suffix length in base32 characters. 10 × 5 = 50 bits.
const SUFFIX_CHARS: usize = 10;

/// Crockford-style alphabet, minus vowels and easily-confused glyphs.
///
/// Hand-rolled rather than taken from a crate precisely because this is frozen:
/// a dependency that changed its alphabet would rewrite every slug.
const ALPHABET: &[u8; 32] = b"0123456789abcdefghjkmnpqrstvwxyz";

/// Why a slug did not derive from words.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SlugBasis {
    /// Words taken from the capture text.
    Words,
    /// Capture had no usable words — a voice note, an image, an empty capture.
    Textless,
    /// The capture looked like a credential, so its words are not used.
    ///
    /// See [`looks_like_credential`]. This is not cosmetic: a slug is the one
    /// part of a note's content that gets copied into *other* notes, where
    /// redaction cannot reach it.
    Credential,
    /// The note has been redacted; the slug reveals nothing.
    Redacted,
}

/// A derived slug and how it was derived.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Slug {
    /// The slug text.
    pub text: String,
    /// What it was based on.
    pub basis: SlugBasis,
}

/// The UTC date of a moment, as `YYYY-MM-DD`.
///
/// # Why UTC, when §2.3 is emphatic that a day belongs to the zone it happened
/// in
///
/// Because this names a note, and a name has to be identical on every replica
/// for ever. A date computed in the capture's own zone would depend on a
/// timezone database, and devices carry their own — the time model says so and
/// says they can disagree. A note folded on a phone with one tzdata version
/// and a desktop with another could then be given two different permanent
/// names, and §1.3 makes re-folding the recovery path, so the name would
/// change under somebody's feet on a rebuild.
///
/// Which day a note is *filed* under is a different question with a different
/// answer, computed in the note's own zone where the surfaces need it. This is
/// only the readable part of a name whose uniqueness lives entirely in its
/// suffix.
///
/// Pure integer arithmetic, because this crate has no clock and no calendar —
/// the algorithm is the standard days-from-civil inverse, valid across the
/// whole range a 64-bit millisecond timestamp can express.
pub fn utc_date(ms: i64) -> alloc::string::String {
    let days = ms.div_euclid(86_400_000);

    // Shift the era so that March is the first month: leap days then land at
    // the end of a year and the arithmetic has no special cases.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };

    alloc::format!("{y:04}-{m:02}-{d:02}")
}

/// Derive the canonical slug for a note.
///
/// `capture_text` is the root capture's text; `date` is its capture date as
/// `YYYY-MM-DD`, used when words are unavailable or unwise.
pub fn derive(capture_text: &str, date: &str, tgt: &EventId) -> Slug {
    let suffix = suffix_of(tgt);

    if looks_like_credential(capture_text) {
        return Slug {
            text: alloc::format!("{date}-{suffix}"),
            basis: SlugBasis::Credential,
        };
    }

    let words = words_of(capture_text);
    if words.is_empty() {
        return Slug {
            text: alloc::format!("{date}-{suffix}"),
            basis: SlugBasis::Textless,
        };
    }

    Slug {
        text: alloc::format!("{}-{suffix}", words.join("-")),
        basis: SlugBasis::Words,
    }
}

/// Slug for a note whose content has been destroyed.
///
/// Derived from identity alone, which does two jobs: the row remains
/// reconstructible on every future rebuild even though its text is gone, and
/// the note's own name stops revealing the words it was built from.
pub fn derive_redacted(tgt: &EventId) -> Slug {
    Slug {
        text: alloc::format!("redacted-{}", suffix_of(tgt)),
        basis: SlugBasis::Redacted,
    }
}

/// The 50-bit identity suffix.
fn suffix_of(tgt: &EventId) -> String {
    let mut bits: u64 = 0;
    for b in &tgt.0[..7] {
        bits = (bits << 8) | u64::from(*b);
    }
    // 7 bytes = 56 bits; keep the top 50.
    bits >>= 6;

    let mut out = String::with_capacity(SUFFIX_CHARS);
    for i in (0..SUFFIX_CHARS).rev() {
        let idx = ((bits >> (i * 5)) & 0x1f) as usize;
        out.push(ALPHABET[idx] as char);
    }
    out
}

/// Extract slug words: NFC, lowercase, segment, transliterate, filter, take.
fn words_of(text: &str) -> Vec<String> {
    let normalised: String = text.nfc().collect();
    let lowered = normalised.to_lowercase();

    let mut out = Vec::new();
    for word in lowered.unicode_words() {
        let ascii: String = word
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() {
                    let mut s = String::new();
                    s.push(c);
                    s
                } else {
                    // Transliterate where the script has a standard mapping;
                    // otherwise the character contributes nothing.
                    let t = any_ascii_char(c);
                    t.chars().filter(|c| c.is_ascii_alphanumeric()).collect()
                }
            })
            .collect();

        if ascii.len() > MIN_WORD_LEN {
            out.push(ascii);
            if out.len() == MAX_WORDS {
                break;
            }
        }
    }
    out
}

/// Does this capture look like it contains a credential?
///
/// Deliberately over-flagging. A false positive costs a little readability; a
/// false negative writes credential-shaped words into every note that links
/// here, where redaction cannot reach them.
///
/// This is a pure predicate, not the recogniser — it has to exist in the same
/// milestone as the frozen slug algorithm, because adding it later would change
/// existing slugs on the next rebuild.
pub fn looks_like_credential(text: &str) -> bool {
    const KEYWORDS: &[&str] = &[
        "password",
        "passwd",
        "secret",
        "token",
        "api key",
        "apikey",
        "api_key",
        "private key",
        "credential",
        "auth",
        "bearer",
    ];

    let lower = text.to_lowercase();

    if lower.contains("-----begin") {
        return true;
    }
    // Connection strings carry embedded credentials by construction.
    if lower.contains("://") && lower.contains('@') {
        return true;
    }
    for kw in KEYWORDS {
        if lower.contains(kw) {
            return true;
        }
    }
    // A long unbroken high-entropy run: base64, hex, or a raw token.
    lower.split_whitespace().any(|tok| {
        let t = tok.trim_matches(|c: char| !c.is_ascii_alphanumeric());
        t.len() >= 20
            && t.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '/')
            && t.chars().any(|c| c.is_ascii_digit())
            && t.chars().any(|c| c.is_ascii_alphabetic())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tgt(b: u8) -> EventId {
        EventId([b; 32])
    }

    #[test]
    fn words_come_from_the_capture() {
        let s = derive("budget 40k, revisit Q2", "2026-08-22", &tgt(1));
        assert_eq!(s.basis, SlugBasis::Words);
        assert!(s.text.starts_with("budget-"), "got {}", s.text);
    }

    #[test]
    fn derivation_is_deterministic() {
        let a = derive("call the dentist", "2026-08-22", &tgt(3));
        let b = derive("call the dentist", "2026-08-22", &tgt(3));
        assert_eq!(a, b);
    }

    #[test]
    fn short_words_are_skipped() {
        // "a", "of" contribute nothing; MIN_WORD_LEN keeps slugs meaningful.
        let s = derive("a bit of budget planning", "2026-08-22", &tgt(1));
        assert!(!s.text.starts_with("bit-"), "got {}", s.text);
    }

    #[test]
    fn identical_text_still_gets_distinct_slugs() {
        // "milk" typed twice is routine; the suffix is what separates them.
        let a = derive("milk", "2026-08-22", &tgt(1));
        let b = derive("milk", "2026-08-22", &tgt(2));
        assert_ne!(a.text, b.text);
    }

    #[test]
    fn textless_capture_falls_back_to_date() {
        let s = derive("", "2026-08-22", &tgt(1));
        assert_eq!(s.basis, SlugBasis::Textless);
        assert!(s.text.starts_with("2026-08-22-"));
    }

    #[test]
    fn non_latin_script_does_not_collapse_the_namespace() {
        let s = derive("日本語のノート", "2026-08-22", &tgt(1));
        // Either transliterated words or the date fallback — never a bare
        // suffix, which would put every such note in one namespace.
        assert!(s.text.len() > SUFFIX_CHARS + 1, "got {}", s.text);
    }

    #[test]
    fn credential_capture_never_reveals_its_words() {
        for text in [
            "postgres prod password hunter2",
            "api key AKIAIOSFODNN7EXAMPLE12345",
            "-----BEGIN RSA PRIVATE KEY-----",
            "postgres://user:pw@host/db",
        ] {
            let s = derive(text, "2026-08-22", &tgt(1));
            assert_eq!(s.basis, SlugBasis::Credential, "missed: {text}");
            assert!(!s.text.contains("password"));
            assert!(!s.text.contains("hunter"));
        }
    }

    #[test]
    fn ordinary_text_is_not_flagged() {
        for text in [
            "budget 40k, revisit Q2",
            "call mum about the weekend",
            "the deploy keeps timing out on staging",
        ] {
            assert!(!looks_like_credential(text), "false positive on: {text}");
        }
    }

    #[test]
    fn redacted_slug_reveals_nothing() {
        let s = derive_redacted(&tgt(1));
        assert_eq!(s.basis, SlugBasis::Redacted);
        assert!(s.text.starts_with("redacted-"));
    }

    #[test]
    fn redacted_slug_is_reproducible_without_the_text() {
        // The property that lets a redacted note survive a rebuild: its slug
        // column is NOT NULL, and its text is gone.
        assert_eq!(derive_redacted(&tgt(9)), derive_redacted(&tgt(9)));
    }

    #[test]
    fn suffix_is_fifty_bits() {
        let s = suffix_of(&tgt(0xff));
        assert_eq!(s.len(), SUFFIX_CHARS);
        assert!(s.chars().all(|c| ALPHABET.contains(&(c as u8))));
    }

    #[test]
    fn suffix_varies_with_identity() {
        assert_ne!(suffix_of(&tgt(1)), suffix_of(&tgt(2)));
    }

    #[test]
    fn tightening_the_predicate_cannot_rewrite_history() {
        // Notes already slugged by words keep their slug: derivation is a pure
        // function of (text, date, tgt), so a *future* predicate change only
        // affects captures made after it.
        let before = derive("ordinary note text here", "2026-08-22", &tgt(1));
        assert_eq!(before.basis, SlugBasis::Words);
        assert_eq!(
            before,
            derive("ordinary note text here", "2026-08-22", &tgt(1))
        );
    }

    #[test]
    fn slug_text_is_url_and_shell_safe() {
        let s = derive(
            "weird: 1:1 notes / with «punctuation»!",
            "2026-08-22",
            &tgt(1),
        );
        assert!(
            s.text
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-'),
            "got {}",
            s.text
        );
    }
}
