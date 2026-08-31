//! Preferences, and the difference between a default and a decision.
//!
//! # A default is a rule; a setting is a decision (§8.9)
//!
//! Defaults are compiled in and **never written to the log on first run**.
//! Materialising them as recorded choices would freeze today's judgement
//! permanently — a later, better default could never reach anybody who had
//! already run the program once — and it would destroy the distinction between
//! a value somebody picked and one they inherited. `capture status` says which is
//! which, and it can only do that because the unchosen case is an absence.
//!
//! Reverting is a third state and not the same as never having chosen: it is
//! stored as a choice with no value. A future change to the default should
//! follow a revert and should not overrule a choice.
//!
//! # Why the table lives here
//!
//! Because [`validate`] reads it, and `set_setting` calls that. A setting is an
//! event: a key or a value this build cannot use, written once, is in the log
//! for ever. The check therefore has to sit where every surface passes, and the
//! table has to sit beside the check. It lived in the command line until a
//! second surface needed it, which is the same shape of mistake as four other
//! operations found the same day.

use crate::projection::Store;
use crate::{Error, Result};

/// What a setting's value is allowed to be.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Kind {
    /// Any text at all.
    Text,
    /// A number, with the range it is useful over.
    ///
    /// Bounded rather than merely parsed. A scale of 40 is not a preference,
    /// it is a window nobody can dismiss — and because settings are events,
    /// the way out would be editing a log that is supposed to be append-only.
    Number { min: f32, max: f32 },
}

/// One preference this build understands.
#[derive(Clone, Copy, Debug)]
pub struct Setting {
    pub key: &'static str,
    /// The value used when nobody has chosen one.
    pub default: &'static str,
    /// What it does, in the words `capture status` shows.
    pub what: &'static str,
    pub kind: Kind,
}

/// Every preference, with its default.
pub const KNOWN: &[Setting] = &[
    Setting {
        key: "timeline.limit",
        default: "50",
        what: "how many notes `capture timeline` shows",
        kind: Kind::Number {
            min: 1.0,
            max: 1000.0,
        },
    },
    Setting {
        key: "capture.text_scale",
        default: "1.35",
        // One knob rather than four. The overlay's sizes were chosen against
        // each other — the field larger than the headings, the headings larger
        // than the legend — and a scale keeps those relationships while making
        // the whole thing legible. Four separate sizes would let somebody make
        // the legend bigger than the thing they are typing.
        what: "how large the capture overlay's text is",
        kind: Kind::Number { min: 0.8, max: 2.5 },
    },
];

/// The setting with this key, if this build knows it.
pub fn known(key: &str) -> Option<&'static Setting> {
    KNOWN.iter().find(|s| s.key == key)
}

/// The compiled-in value for a key.
pub fn default_for(key: &str) -> Option<&'static str> {
    known(key).map(|s| s.default)
}

/// Whether this key and value may be written.
///
/// Both halves matter and only the first was ever checked. A setting is an
/// event: `capture.text_scale = "huge"` written once is in the log for ever,
/// unusable by every build that reads it, and silently falling back to the
/// default at read time would leave somebody looking at a setting they had
/// apparently chosen and that plainly did nothing.
pub fn validate(key: &str, value: Option<&str>) -> Result<()> {
    let Some(setting) = known(key) else {
        let keys: Vec<&str> = KNOWN.iter().map(|s| s.key).collect();
        return Err(Error::Usage(format!(
            "'{key}' is not a setting this build knows.\n\nKnown: {}",
            keys.join(", ")
        )));
    };

    // Reverting to the default is always allowed: it is a decision to stop
    // deciding, and it cannot be out of range.
    let Some(value) = value else { return Ok(()) };

    match setting.kind {
        Kind::Text => Ok(()),
        Kind::Number { min, max } => {
            let n: f32 = value
                .trim()
                .parse()
                .map_err(|_| Error::Usage(format!("'{value}' is not a number; {key} takes one")))?;
            if !(min..=max).contains(&n) {
                return Err(Error::Usage(format!(
                    "{key} takes a number between {min} and {max}; {n} is outside that"
                )));
            }
            Ok(())
        }
    }
}

/// The value in force: what was chosen, or what is compiled in.
pub fn effective(state: &Store, key: &str) -> Result<Option<String>> {
    for (k, v, _) in state.settings()? {
        if k == key {
            // An explicit revert is stored as a choice with no value and takes
            // the default, which is why this falls through rather than
            // returning `None` as "unset".
            return Ok(v.or_else(|| default_for(key).map(String::from)));
        }
    }
    Ok(default_for(key).map(String::from))
}

/// The value in force, as a number.
///
/// Falls back to the compiled-in default when the store cannot be read at all —
/// a surface that failed to start because a preference was unavailable would be
/// trading a legible window for no window.
pub fn number(state: Option<&Store>, key: &str) -> f32 {
    let fallback = || default_for(key).and_then(|d| d.parse().ok()).unwrap_or(1.0);

    let Some(state) = state else {
        return fallback();
    };
    match effective(state, key) {
        Ok(Some(v)) => v.trim().parse().unwrap_or_else(|_| fallback()),
        _ => fallback(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_default_is_a_value_its_own_kind_accepts() {
        // A default that would be refused if somebody typed it is a default
        // nobody can restore, and the failure appears only when they try.
        for s in KNOWN {
            validate(s.key, Some(s.default))
                .unwrap_or_else(|e| panic!("the default for {} is not valid: {e}", s.key));
        }
        assert!(
            !KNOWN.is_empty(),
            "there are no settings, so this checks nothing"
        );
    }

    #[test]
    fn a_value_this_build_cannot_use_is_refused() {
        assert!(validate("capture.text_scale", Some("huge")).is_err());
        assert!(validate("capture.text_scale", Some("40")).is_err());
        assert!(validate("capture.text_scale", Some("1.6")).is_ok());
    }

    #[test]
    fn reverting_is_always_allowed() {
        // It is a decision to stop deciding, and it cannot be out of range.
        assert!(validate("capture.text_scale", None).is_ok());
    }

    #[test]
    fn an_unknown_key_is_refused_with_the_ones_that_exist() {
        let e = validate("capture.colour", Some("green")).unwrap_err();
        let msg = e.to_string();
        assert!(msg.contains("not a setting"), "{msg}");
        assert!(
            msg.contains("capture.text_scale"),
            "the refusal lists nothing usable: {msg}"
        );
    }
}
