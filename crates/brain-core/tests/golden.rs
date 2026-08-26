//! Golden vectors for every frozen derivation.
//!
//! The crate choices make drift *unlikely*. This file makes it **detectable** —
//! and it is the only thing that does. Neither substitutes for the other.
//!
//! Every constant below is a commitment. If a change to this crate alters one,
//! that is not a test to update: it means existing event ids no longer
//! recompute, existing signatures no longer verify, and every hand-written link
//! to an existing note has broken. The correct response is to revert the change
//! or to declare a deliberate, converted fork.
//!
//! The scenario this guards against is mundane: someone runs `cargo update`,
//! `unicode-segmentation` ships new tables, word boundaries shift for some
//! inputs, and slugs silently change on the next rebuild. Nothing else in the
//! system would notice.
//!
//! CI runs these on more than one platform, because "the same logical event
//! encodes identically on two platforms" is a claim about two platforms and
//! cannot be checked from one.

use brain_core::{
    Dev, EventId, Incarnation, SLUG_VERSION,
    body::{
        Ambient, CaptureBody, Cue, EventFields, IntentBody, OccBody, OccEditBody, OccMoveBody,
        Outcome, PromoteBody, Promoted, ReviewBody, RuleSpec, SeriesFields, TaskFields, When,
    },
    event::{ENVELOPE_VERSION, Header, Kind},
    hlc::Hlc,
    id::BodyHash,
    slug,
};

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// A fixed capture body. Every field is pinned so the encoding is reproducible.
fn fixture_body() -> CaptureBody {
    CaptureBody {
        nonce: [0xab; 16],
        text: "budget 40k, revisit Q2".to_string(),
        tz: Some("Europe/London".to_string()),
        ambient: Ambient {
            app: None,
            window_title: None,
            cwd: Some("/home/u/work/acme".to_string()),
            repo: Some("acme".to_string()),
            branch: Some("rework-auth".to_string()),
            source_url: None,
        },
    }
}

/// Canonical bytes for any body.
fn canonical_of<T: minicbor::Encode<()>>(v: &T) -> Vec<u8> {
    let mut b = Vec::new();
    minicbor::encode(v, &mut b).unwrap();
    b
}

fn canonical_body() -> Vec<u8> {
    let mut b = Vec::new();
    minicbor::encode(fixture_body(), &mut b).unwrap();
    b
}

/// A fixed header over that body.
fn fixture_header() -> Header {
    Header {
        v: ENVELOPE_VERSION,
        dev: Dev::derive(&[0x11; 32], &Incarnation([0x22; 16])),
        seq: 41822,
        hlc: Hlc {
            wall: 1_755_710_241_371,
            counter: 3,
            origin: Dev::derive(&[0x11; 32], &Incarnation([0x22; 16])),
        },
        kind: Kind::CAPTURE,
        tgt: None,
        prev: None,
        occ: None,
        chain: None,
        bhash: BodyHash::of(&canonical_body()),
    }
}

#[test]
fn envelope_version_is_pinned() {
    assert_eq!(
        ENVELOPE_VERSION, 1,
        "changing the envelope version is a deliberate fork, not an edit"
    );
}

#[test]
fn slug_version_is_pinned() {
    assert_eq!(SLUG_VERSION, 1);
}

#[test]
fn origin_derivation_is_frozen() {
    let dev = Dev::derive(&[0x11; 32], &Incarnation([0x22; 16]));
    assert_eq!(
        hex(&dev.0),
        "cf8a978a4b47004f9fbf6be7195f9d89beaf6d9d1a9f2cc50bd7bfa6ed741fa1"
    );
}

#[test]
fn body_encoding_is_frozen() {
    assert_eq!(
        hex(&canonical_body()),
        "8450abababababababababababababababab766275646765742034306b2c20726576697369742051326d4575726f70652f4c6f6e646f6e85f6f6712f686f6d652f752f776f726b2f61636d656461636d656b7265776f726b2d61757468"
    );
}

#[test]
fn body_commitment_is_frozen() {
    assert_eq!(
        hex(&BodyHash::of(&canonical_body()).0),
        "b37e53bf6e7d04e897bcad978469d50804a20c8ea5417a1f6a67ae3b249c68fc"
    );
}

#[test]
fn event_id_is_frozen() {
    assert_eq!(
        hex(&fixture_header().id().0),
        "fbc1eae192e80d797b2982481010c2c8f2c010aa5c13371c1f59c26419b46674"
    );
}

#[test]
fn slug_derivation_is_frozen() {
    let cases: &[(&str, &str)] = &[
        ("budget 40k, revisit Q2", "budget-revisit"),
        ("call mum about the weekend", "call-about-weekend"),
        (
            "the deploy keeps timing out on staging",
            "deploy-keeps-timing-staging",
        ),
    ];
    for (text, expected_words) in cases {
        let s = slug::derive(text, "2026-08-22", &EventId([0x01; 32]));
        let got_words = s.text.rsplit_once('-').unwrap().0;
        assert_eq!(
            got_words, *expected_words,
            "slug word selection changed for {text:?} — every link to such a note has broken"
        );
    }
}

#[test]
fn credential_predicate_is_frozen() {
    // Flagged. A change here that *unflags* any of these is a leak.
    for text in [
        "postgres prod password hunter2",
        "api key AKIAIOSFODNN7EXAMPLE12345",
        "-----BEGIN RSA PRIVATE KEY-----",
        "postgres://user:pw@host/db",
        "bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9",
    ] {
        assert!(
            slug::looks_like_credential(text),
            "no longer flagged: {text}"
        );
    }

    // Not flagged. A change that *starts* flagging these costs readability
    // only — the safe direction — but is still a slug change, so it belongs in
    // a version bump rather than arriving silently.
    for text in [
        "budget 40k, revisit Q2",
        "call mum about the weekend",
        "the deploy keeps timing out on staging",
    ] {
        assert!(!slug::looks_like_credential(text), "newly flagged: {text}");
    }
}

#[test]
fn suffix_is_stable_for_an_identity() {
    let a = slug::derive("some ordinary text", "2026-08-22", &EventId([0x07; 32]));
    let b = slug::derive("some ordinary text", "2026-08-22", &EventId([0x07; 32]));
    assert_eq!(a.text, b.text);
}

// ---------------------------------------------------------------------------
// M2 bodies.
//
// Written before anything authors one of these, which is the only moment they
// can be written. A body shape is permanent from its first use — `ENVELOPE_VERSION`
// does not cover it, and a breaking change needs a whole new kind rather than a
// version bump. So a field forgotten here is not a bug to fix later; it is a
// field that can never be added.
// ---------------------------------------------------------------------------

fn promote_task() -> PromoteBody {
    PromoteBody {
        nonce: [0x11; 16],
        as_: Promoted::Task(TaskFields {
            done: false,
            due: Some("2026-08-14".to_string()),
            order: Some(3),
        }),
        pinned: true,
        grammar: None,
        confidence: None,
    }
}

fn promote_series() -> PromoteBody {
    PromoteBody {
        nonce: [0x22; 16],
        as_: Promoted::Series(SeriesFields {
            when: When::Timed {
                start_ms: 1_755_781_200_000,
                end_ms: Some(1_755_784_800_000),
                tz: "America/Denver".to_string(),
            },
            rule: RuleSpec {
                freq: 1,
                interval: 1,
                by_day: 0b0001_1111,
                by_month_day: vec![],
                count: None,
                until: None,
            },
            carries_forward: false,
            split_from: None,
        }),
        pinned: false,
        grammar: Some(1),
        confidence: Some(80),
    }
}

fn promote_all_day() -> PromoteBody {
    PromoteBody {
        nonce: [0x33; 16],
        as_: Promoted::Event(EventFields {
            when: When::AllDay {
                start_date: "2026-08-01".to_string(),
                end_date: "2026-08-14".to_string(),
            },
            location: None,
            attendees: vec![],
        }),
        pinned: false,
        grammar: Some(1),
        confidence: Some(85),
    }
}

#[test]
fn promote_encoding_is_frozen() {
    assert_eq!(
        hex(&canonical_of(&promote_task())),
        "83501111111111111111111111111111111182018183f46a323032362d30382d313403f5"
    );
    assert_eq!(
        hex(&canonical_of(&promote_series())),
        "855022222222222222222222222222222222820381838200831b00000198ccb6f8801b00000198ccede7006e416d65726963612f44656e766572840101181f80f4f4011850"
    );
    assert_eq!(
        hex(&canonical_of(&promote_all_day())),
        "855033333333333333333333333333333333820281838201826a323032362d30382d30316a323032362d30382d3134f680f4011855"
    );
}

#[test]
fn a_trailing_absent_field_is_simply_absent() {
    // The task vector is a three-element array and the series vector is a
    // five-element one, from the same struct. Trailing `None`s are not encoded
    // at all, which is deterministic and worth pinning: it means adding a field
    // later would change the encoding of every body that omitted it, and there
    // is no version bump that could cover that (see the module header).
    let task = canonical_of(&promote_task());
    let series = canonical_of(&promote_series());
    assert_eq!(task[0], 0x83, "expected a three-element array");
    assert_eq!(series[0], 0x85, "expected a five-element array");
}

#[test]
fn intent_encoding_is_frozen() {
    let b = IntentBody {
        nonce: [0x44; 16],
        cue: Cue::AppFocus("Slack".to_string()),
        then: "check the #incidents backlog before anything else".to_string(),
    };
    assert_eq!(
        hex(&canonical_of(&b)),
        "83504444444444444444444444444444444482038165536c61636b7831636865636b207468652023696e636964656e7473206261636b6c6f67206265666f726520616e797468696e6720656c7365"
    );
}

#[test]
fn a_geofence_encodes_as_exact_integers() {
    // Micro-degrees rather than floats, because this body is hashed into the
    // event id: a float would make that id depend on two devices agreeing
    // bit-for-bit about a decimal fraction, and would put NaN — not equal to
    // itself — inside a value that has to converge.
    let b = IntentBody {
        nonce: [0x99; 16],
        cue: Cue::Location {
            lat_e6: 51_507_351,
            lon_e6: -127_758,
            radius_m: 150,
        },
        then: "buy milk".to_string(),
    };
    assert_eq!(
        hex(&canonical_of(&b)),
        "8350999999999999999999999999999999998202831a0311f0973a0001f30d189668627579206d696c6b"
    );
}

#[test]
fn review_encoding_is_frozen() {
    // Three outcomes, and no fourth. `recalled / missed / snoozed` was
    // withdrawn in §2.7, and this is where that becomes unavailable rather than
    // merely discouraged — a graded outcome has no representation here.
    for (outcome, expected) in [
        (Outcome::Shown, "825055555555555555555555555555555555820080"),
        (
            Outcome::Engaged,
            "825055555555555555555555555555555555820180",
        ),
        (
            Outcome::Deferred,
            "825055555555555555555555555555555555820280",
        ),
    ] {
        let b = ReviewBody {
            nonce: [0x55; 16],
            outcome,
        };
        assert_eq!(hex(&canonical_of(&b)), expected, "{outcome:?}");
    }
}

#[test]
fn occurrence_encodings_are_frozen() {
    let moved = OccMoveBody {
        nonce: [0x66; 16],
        occ: 1_755_781_200_000,
        start_ms: 1_755_788_400_000,
        end_ms: None,
    };
    assert_eq!(
        hex(&canonical_of(&moved)),
        "8350666666666666666666666666666666661b00000198ccb6f8801b00000198cd24d580"
    );

    let edited = OccEditBody {
        nonce: [0x77; 16],
        occ: 1_755_781_200_000,
        title: Some("moved to the small room".to_string()),
        location: None,
        notes: None,
    };
    assert_eq!(
        hex(&canonical_of(&edited)),
        "8350777777777777777777777777777777771b00000198ccb6f880776d6f76656420746f2074686520736d616c6c20726f6f6d"
    );

    // Cancel, done and undone share one shape: which occurrence, and nothing
    // else. Each is a different kind, so the meaning is in the kind rather than
    // in a discriminator inside the body.
    let plain = OccBody {
        nonce: [0x88; 16],
        occ: 1_755_781_200_000,
    };
    assert_eq!(
        hex(&canonical_of(&plain)),
        "8250888888888888888888888888888888881b00000198ccb6f880"
    );
}

#[test]
fn the_occurrence_name_survives_encoding_exactly() {
    // `occ` is the identity half of `(series_id, original_start)`. A rounding
    // or precision change here would silently orphan every exception ever
    // written, because they would stop naming the occurrence they were about.
    let plain = OccBody {
        nonce: [0u8; 16],
        occ: i64::MAX,
    };
    let back: OccBody = minicbor::decode(&canonical_of(&plain)).unwrap();
    assert_eq!(back.occ, i64::MAX);

    let negative = OccBody {
        nonce: [0u8; 16],
        occ: -1_000_000,
    };
    let back: OccBody = minicbor::decode(&canonical_of(&negative)).unwrap();
    assert_eq!(
        back.occ, -1_000_000,
        "an occurrence before 1970 must survive"
    );
}

#[test]
fn every_m2_body_round_trips() {
    // Task 2.4: encode, decode, and get back exactly what went in. The id is
    // taken over the body hash, so a body that does not round-trip is an event
    // that cannot be verified after it is read back.
    macro_rules! round_trip {
        ($v:expr, $t:ty) => {{
            let original = $v;
            let bytes = canonical_of(&original);
            let back: $t = minicbor::decode(&bytes).expect("decodes");
            assert_eq!(back, original);
            assert_eq!(canonical_of(&back), bytes, "re-encoding differed");
        }};
    }

    round_trip!(promote_task(), PromoteBody);
    round_trip!(promote_series(), PromoteBody);
    round_trip!(promote_all_day(), PromoteBody);
    round_trip!(
        PromoteBody {
            nonce: [1; 16],
            as_: Promoted::Note,
            pinned: true,
            grammar: None,
            confidence: None,
        },
        PromoteBody
    );
    round_trip!(
        IntentBody {
            nonce: [2; 16],
            cue: Cue::DeviceWake,
            then: "x".to_string(),
        },
        IntentBody
    );
    round_trip!(
        IntentBody {
            nonce: [3; 16],
            cue: Cue::Recurring(RuleSpec {
                freq: 0,
                interval: 2,
                by_day: 0,
                by_month_day: vec![1, 15],
                count: Some(10),
                until: Some(99),
            }),
            then: "y".to_string(),
        },
        IntentBody
    );
    round_trip!(
        IntentBody {
            nonce: [4; 16],
            cue: Cue::CalendarStart(None),
            then: "z".to_string(),
        },
        IntentBody
    );
    round_trip!(
        IntentBody {
            nonce: [5; 16],
            cue: Cue::GitBranch("main".to_string()),
            then: "w".to_string(),
        },
        IntentBody
    );
    round_trip!(
        ReviewBody {
            nonce: [6; 16],
            outcome: Outcome::Engaged,
        },
        ReviewBody
    );
    round_trip!(
        OccMoveBody {
            nonce: [7; 16],
            occ: 1,
            start_ms: 2,
            end_ms: Some(3),
        },
        OccMoveBody
    );
    round_trip!(
        OccEditBody {
            nonce: [8; 16],
            occ: 1,
            title: None,
            location: Some("room 2".to_string()),
            notes: Some("bring the laptop".to_string()),
        },
        OccEditBody
    );
    round_trip!(
        OccBody {
            nonce: [9; 16],
            occ: 1
        },
        OccBody
    );
}
