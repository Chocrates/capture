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

use capture_core::{
    Dev, EventId, Incarnation, SLUG_VERSION,
    body::{
        AliasBody, Ambient, CaptureBody, Cue, EditBody, EventFields, IntentBody, MuteBody,
        MuteScope, OccBody, OccEditBody, OccMoveBody, OriginClaimBody, Outcome, ParentBody,
        PromoteBody, Promoted, ReadmitBody, RedactBody, RedactScope, RetireBody, RetiredHead,
        ReviewBody, RosterKeyBody, RuleSpec, SeriesFields, SettingBody, TagBody, TaskFields, When,
        readmit_digest,
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
        blob: None,
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
        released: None,
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
        released: None,
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
        released: None,
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
            released: None,
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

#[test]
fn a_parent_encodes_the_same_way_for_ever() {
    // Frozen before anything writes one, like every other body here. A note
    // filed under another is a `promote`-shaped decision — small, reversible,
    // and permanent in the log — so the encoding has to be settled before the
    // first one exists rather than after.
    use capture_core::id::EventId;

    let under = ParentBody {
        nonce: [0x99; 16],
        parent: Some(EventId([0x11; 32])),
    };
    assert_eq!(
        hex(&canonical_of(&under)),
        "825099999999999999999999999999999999582011111111111111111111111111111111111111111111111111111111111111\
11"
    );

    // Back to the top, and the encoding of it is worth writing down because it
    // is not what it looks like: a trailing `None` is elided entirely, so this
    // is a one-element array rather than a two-element one ending in null. The
    // assertion here was written the other way round first and this test caught
    // it, which is the reason the vectors are frozen before anything writes
    // one.
    //
    // It is safe *here*, and the reason is specific rather than general. The
    // meaning lives in the kind: a `parent` event with no parent in it says
    // "under nothing", because there is no other thing such an event could be
    // saying. So it still decodes to `None`, still beats an earlier parent by
    // ordinary last-write-wins, and "moved back to the top" survives the round
    // trip. A field added after this one would need care — that is true of
    // every body in this file and is why they are all frozen.
    let top = ParentBody {
        nonce: [0x99; 16],
        parent: None,
    };
    let bytes = canonical_of(&top);
    let back: ParentBody = minicbor::decode(&bytes).unwrap();
    assert_eq!(back.parent, None, "moving back to the top did not survive");
    assert_eq!(hex(&bytes), "815099999999999999999999999999999999");
}

#[test]
fn a_membership_claim_encodes_the_same_way_for_ever() {
    // The body that admits a device, and arguably the most frozen thing here:
    // get it wrong and a device is locked out of its own archive permanently,
    // because membership is what ingest checks before it will accept anything.
    //
    // It had no vector until sync needed a field added to it, which is the gap
    // worth naming rather than quietly closing — the discipline was applied to
    // every body M2 introduced and never retrofitted to the ones M1 wrote.
    let old = OriginClaimBody {
        nonce: [0x11; 16],
        pubkey: [0x22; 32],
        incarnation: [0x33; 16],
        noise: None,
        role: None,
        name: None,
    };

    // Three fields, not four. A trailing `None` is elided entirely, which is
    // what makes adding a field at the end safe: every claim written before
    // sync existed encodes exactly as it did, byte for byte.
    // 83 array(3) · 50 nonce(16) · 5820 pubkey(32) · 50 incarnation(16), and
    // nothing after it.
    assert_eq!(
        hex(&canonical_of(&old)),
        "835011111111111111111111111111111111582022222222222222222222222222222222222222222222222222222222222222225033333333333333333333333333333333"
    );

    let with_noise = OriginClaimBody {
        noise: Some([0x44; 32]),
        ..old.clone()
    };
    // 84 array(4) now, with the key appended: the only change a frozen body
    // permits, and visibly a suffix rather than a rearrangement.
    assert_eq!(
        hex(&canonical_of(&with_noise)),
        "84501111111111111111111111111111111158202222222222222222222222222222222222222222222222222222222222222222503333333333333333333333333333333358204444444444444444444444444444444444444444444444444444444444444444"
    );

    // And an older claim still reads, which is the property the whole
    // append-at-the-end rule exists to give.
    let back: OriginClaimBody = minicbor::decode(&canonical_of(&old)).unwrap();
    assert_eq!(back.noise, None);
    assert_eq!(back.pubkey, [0x22; 32]);
}

#[test]
fn an_agent_admission_is_a_suffix_an_older_build_reads_past() {
    let agent = OriginClaimBody {
        nonce: [0x11; 16],
        pubkey: [0x22; 32],
        incarnation: [0x33; 16],
        noise: None,
        role: Some(capture_core::body::ROLE_AGENT),
        name: Some("claude".into()),
    };
    let bytes = canonical_of(&agent);
    // 86 array(6): the three it always had, f6 (no noise key: an agent is
    // never dialled), 01 the role, 66 "claude".
    assert_eq!(
        hex(&bytes),
        "865011111111111111111111111111111111582022222222222222222222222222222222222222222222222222222222222222225033333333333333333333333333333333f60166636c61756465"
    );
    let back: OriginClaimBody = minicbor::decode(&bytes).unwrap();
    assert_eq!(back, agent);

    // The claim as a build before agents knew it: four fields. It must decode
    // the new bytes rather than refuse them, or an older peer would hold every
    // event the agent wrote.
    #[derive(minicbor::Decode)]
    struct Before {
        #[cbor(n(0), with = "minicbor::bytes")]
        _nonce: [u8; 16],
        #[cbor(n(1), with = "minicbor::bytes")]
        pubkey: [u8; 32],
        #[cbor(n(2), with = "minicbor::bytes")]
        _incarnation: [u8; 16],
        #[cbor(n(3), with = "minicbor::bytes")]
        noise: Option<[u8; 32]>,
    }
    let old: Before = minicbor::decode(&bytes).expect("an older build reads past the role");
    assert_eq!(old.pubkey, [0x22; 32]);
    assert_eq!(old.noise, None);
}

// ---------------------------------------------------------------------------
// The bodies M1 wrote, which had no vectors until now.
//
// The discipline was applied to every body M2 introduced and never retrofitted
// to these, which is how a field was added to `OriginClaimBody` with nothing
// objecting. Every store in existence already holds these encodings, and the
// recovery story for all of them is "re-fold the log" — an encoding that drifts
// silently makes that story false while leaving every test green.
// ---------------------------------------------------------------------------

#[test]
fn an_edit_encodes_the_same_way_for_ever() {
    let b = EditBody {
        nonce: [0x11; 16],
        text: String::from("replaced"),
    };
    assert_eq!(
        hex(&canonical_of(&b)),
        "825011111111111111111111111111111111687265706c61636564"
    );
}

#[test]
fn a_tag_encodes_the_same_way_for_ever() {
    let b = TagBody {
        nonce: [0x11; 16],
        tag: String::from("reading"),
    };
    assert_eq!(
        hex(&canonical_of(&b)),
        "8250111111111111111111111111111111116772656164696e67"
    );
}

#[test]
fn an_alias_encodes_the_same_way_for_ever() {
    let named = AliasBody {
        nonce: [0x11; 16],
        alias: Some(String::from("acme")),
    };
    assert_eq!(
        hex(&canonical_of(&named)),
        "8250111111111111111111111111111111116461636d65"
    );

    // Clearing a name is a decision and has to be distinguishable from never
    // having set one. A trailing `None` is elided, so this is one field —
    // which is safe here because the kind says what the event is.
    let cleared = AliasBody {
        nonce: [0x11; 16],
        alias: None,
    };
    assert_eq!(
        hex(&canonical_of(&cleared)),
        // array(1): the value is gone entirely, not present-and-null.
        "815011111111111111111111111111111111"
    );
}

#[test]
fn a_mute_encodes_the_same_way_for_ever() {
    // Three scopes, and the discriminants are permanent: a mute that decoded as
    // the wrong scope would silence the wrong notes.
    let by_note = MuteBody {
        nonce: [0x11; 16],
        scope: MuteScope::Note(EventId([0x22; 32])),
        until: None,
    };
    assert_eq!(
        hex(&canonical_of(&by_note)),
        "82501111111111111111111111111111111182008158202222222222222222222222222222222222222222222222222222222222222222"
    );

    let by_tag = MuteBody {
        nonce: [0x11; 16],
        scope: MuteScope::Tag(String::from("noisy")),
        until: Some(1_800_000_000_000),
    };
    assert_eq!(
        hex(&canonical_of(&by_tag)),
        "835011111111111111111111111111111111820181656e6f6973791b000001a3185c5000"
    );
}

#[test]
fn a_redaction_encodes_the_same_way_for_ever() {
    // The one irreversible event. Its scope decides whether a note's whole
    // history is destroyed or a single event is, and reading that wrongly
    // destroys the wrong thing.
    let b = RedactBody {
        nonce: [0x11; 16],
        scope: RedactScope::WholeNote,
        targets: vec![EventId([0x22; 32])],
    };
    assert_eq!(
        hex(&canonical_of(&b)),
        // array(3) · nonce(16) · scope [0,[]] = WholeNote · array(1) · target(32)
        "8350111111111111111111111111111111118200808158202222222222222222222222222222222222222222222222222222222222222222"
    );
}

#[test]
fn a_setting_encodes_the_same_way_for_ever() {
    let chosen = SettingBody {
        nonce: [0x11; 16],
        key: String::from("timeline.limit"),
        value: Some(String::from("20")),
    };
    assert_eq!(
        hex(&canonical_of(&chosen)),
        "8350111111111111111111111111111111116e74696d656c696e652e6c696d6974623230"
    );

    // Reverting is a third state, distinct from never having chosen (§8.9), so
    // its encoding has to be distinguishable from an absent field.
    let reverted = SettingBody {
        nonce: [0x11; 16],
        key: String::from("timeline.limit"),
        value: None,
    };
    assert_eq!(
        hex(&canonical_of(&reverted)),
        "8250111111111111111111111111111111116e74696d656c696e652e6c696d6974"
    );
}

/// The roster secret is broadcast-adjacent and long-lived: every device on
/// the mesh has to derive the same pseudonym from it or they stop finding
/// each other, and a device that has been off for a month rejoins against
/// whatever the others are advertising under.
#[test]
fn a_roster_key_encodes_the_same_way_for_ever() {
    let b = RosterKeyBody {
        nonce: [0x11; 16],
        secret: [0x22; 32],
    };
    assert_eq!(
        hex(&canonical_of(&b)),
        // array(2) · nonce(16) · secret(32). No optional tail: a rotation
        // that did not say what the new key is would be meaningless.
        "82501111111111111111111111111111111158202222222222222222222222222222222222222222222222222222222222222222"
    );
}

/// A retirement outlives the device it retires, and has to be readable by
/// every build that comes after — including ones written long after the
/// device is gone.
#[test]
fn a_retirement_encodes_the_same_way_for_ever() {
    let b = RetireBody {
        nonce: [0x11; 16],
        pubkey: [0x22; 32],
        heads: vec![RetiredHead {
            dev: Dev([0x33; 32]),
            seq: 15677,
            last: EventId([0x44; 32]),
        }],
        // Absent, and therefore elided: an ordinary retirement is one array
        // element shorter than a reissue, which is what tells the two apart.
        supersedes: None,
    };
    assert_eq!(
        hex(&canonical_of(&b)),
        // array(3) · nonce(16) · retired key(32) · heads: array(1) of
        // [dev(32), seq, last(32)]. The head is a pair, not a bare sequence
        // number: a number alone could be claimed by a forged event sitting
        // at that position.
        "83501111111111111111111111111111111158202222222222222222222222222222222222222222222222222222222222222222818358203333333333333333333333333333333333333333333333333333333333333333193d3d58204444444444444444444444444444444444444444444444444444444444444444"
    );
}

/// The one event whose validity does not descend from roster membership.
#[test]
fn a_readmission_encodes_the_same_way_for_ever() {
    let b = ReadmitBody {
        nonce: [0x11; 16],
        pubkey: [0x22; 32],
        incarnation: [0x33; 16],
        cosigner: [0x44; 32],
        cosign: [0x55; 64],
    };
    assert_eq!(
        hex(&canonical_of(&b)),
        // array(5) · nonce(16) · subject(32) · incarnation(16) · cosigner(32)
        // · cosignature(64)
        "85501111111111111111111111111111111158202222222222222222222222222222222222222222222222222222222222222222503333333333333333333333333333333358204444444444444444444444444444444444444444444444444444444444444444584055555555555555555555555555555555555555555555555555555555555555555555555555555555555555555555555555555555555555555555555555555555"
    );
}

/// What the second signer signs. Frozen because a co-signature produced by one
/// build has to verify on every other — a device in a lockout is not in a
/// position to upgrade first.
#[test]
fn the_readmission_digest_is_fixed_for_ever() {
    assert_eq!(
        hex(&readmit_digest(&[0x22; 32], &[0x33; 16], &[0x11; 16])),
        "69f8f9d159afc26756ad5b777058e5ca412b4dabdfd08e595e7caf6ed0aba2e7"
    );
}

/// A reissue, which is a retirement plus the one it replaces. Written down
/// separately because the difference between the two shapes is what decides
/// whether a head widens or narrows.
#[test]
fn a_reissued_retirement_encodes_the_same_way_for_ever() {
    let b = RetireBody {
        nonce: [0x11; 16],
        pubkey: [0x22; 32],
        heads: vec![],
        supersedes: Some(EventId([0x55; 32])),
    };
    assert_eq!(
        hex(&canonical_of(&b)),
        // array(4), where an ordinary retirement is array(3): nonce(16) ·
        // key(32) · heads: array(0) · superseded id(32).
        "845011111111111111111111111111111111582022222222222222222222222222222222222222222222222222222222222222228058205555555555555555555555555555555555555555555555555555555555555555"
    );
}

/// A capture carrying a recording (§5.6).
///
/// The pair with the plain capture above is the point. A capture that has no
/// blob encodes exactly as it did before blobs existed — minicbor elides a
/// trailing `None` — so every note already in a log still recomputes its id,
/// still verifies its signature, and every hand-written link to it still
/// resolves. That is not a nicety: §1.3 makes re-folding both the recovery
/// path and the migration strategy, so an encoding change that altered
/// existing ids would make every archive in the world unrecoverable.
#[test]
fn a_capture_with_a_recording_encodes_the_same_way_for_ever() {
    let mut body = fixture_body();
    body.blob = Some(capture_core::body::BlobRef {
        hash: [0x9d; 32],
        media_type: String::from("audio/ogg"),
        bytes: 41822,
        duration_ms: Some(9310),
    });

    assert_eq!(
        hex(&canonical_of(&body)),
        // capture: array(5) — nonce(16) · text · tz · ambient · blob, where
        // the blob is array(4): hash(32) · media type · bytes · duration.
        "8550abababababababababababababababab766275646765742034306b2c20726576697369742051326d4575726f70652f4c6f6e646f6e85f6f6712f686f6d652f752f776f726b2f61636d656461636d656b7265776f726b2d617574688458209d9d9d9d9d9d9d9d9d9d9d9d9d9d9d9d9d9d9d9d9d9d9d9d9d9d9d9d9d9d9d9d69617564696f2f6f676719a35e19245e"
    );

    // And the same capture without one is byte-for-byte what it always was.
    let plain = fixture_body();
    assert_eq!(
        canonical_of(&plain),
        canonical_body(),
        "a capture with no blob must encode as it did before blobs existed"
    );
    assert!(
        canonical_of(&plain).len() < canonical_of(&body).len(),
        "the field costs nothing when absent"
    );

    // A blob is addressed by what it contains, so two captures of the same
    // recording name the same bytes. Nothing here dedupes; the naming makes
    // duplication impossible in the first place.
    let mut again = fixture_body();
    again.text = String::from("a different note about the same recording");
    again.blob = body.blob.clone();
    let (Some(a), Some(b)) = (&body.blob, &again.blob) else {
        panic!("both carry a blob")
    };
    assert_eq!(a.hash, b.hash, "the content decides the name");
}

/// The date a textless note is named for.
///
/// Pinned because it goes into a permanent name. UTC rather than the capture's
/// own zone: a name has to be identical on every replica for ever, and a date
/// computed through a timezone database would depend on which version of that
/// database folded it — devices carry their own and the time model says they
/// can disagree.
#[test]
fn the_date_in_a_name_is_the_same_everywhere_for_ever() {
    use capture_core::slug::utc_date;

    assert_eq!(utc_date(0), "1970-01-01");
    assert_eq!(utc_date(1_788_134_400_000), "2026-08-31");

    // A leap day, and the day after it, because February is where every
    // hand-rolled calendar goes wrong.
    assert_eq!(utc_date(1_709_164_800_000), "2024-02-29");
    assert_eq!(utc_date(1_709_251_200_000), "2024-03-01");

    // A century that is not a leap year, which is the case the simple rule
    // gets wrong and which no test written this decade would otherwise reach.
    assert_eq!(utc_date(4_107_456_000_000), "2100-02-28");
    assert_eq!(utc_date(4_107_542_400_000), "2100-03-01");

    // Before the epoch. A clock that has been set wrong is not a reason to
    // panic or to produce a name nobody can read.
    assert_eq!(utc_date(-1), "1969-12-31");

    // The boundary itself: the last millisecond of a day and the first of the
    // next belong to different days.
    assert_eq!(utc_date(86_399_999), "1970-01-01");
    assert_eq!(utc_date(86_400_000), "1970-01-02");
}

// ----------------------------------------------------------- entities -------
//
// Frozen before anything writes one, which for these is the whole point: an
// `ent_id` is derived rather than minted, so the derivation is load-bearing in a
// way a minted id would not be. If `casefold` or the domain tag changed, every
// entity in every archive would silently become a different entity, every
// `entity_rename` already written would point at nothing, and the only symptom
// would be pages that had quietly emptied.

#[test]
fn an_entity_id_derives_the_same_way_for_ever() {
    use capture_core::id::EntId;

    // The derivation, pinned. `blake3("entity" ‖ kind ‖ 0x00 ‖ casefold(name))`.
    assert_eq!(
        hex(&EntId::derive("person", "sarah").0),
        "234d36238abd9b9dd1e699365adfe14cd005bd71871fb1ef542b2f10538db21a"
    );

    // Case folds into identity. §4.3 wants one person here, not two, and this
    // is the assertion that says so in bytes rather than in prose.
    assert_eq!(
        EntId::derive("person", "Sarah"),
        EntId::derive("person", "sarah"),
        "case forked an entity"
    );
    assert_eq!(
        EntId::derive("project", "MIGRATION"),
        EntId::derive("project", "migration")
    );

    // The kind is in the hash, so a project and a person sharing a word are two
    // entities. They routinely do share one — a client, and the account named
    // after them — and merging those because of the spelling would be the
    // system deciding something nobody said.
    assert_ne!(
        EntId::derive("person", "sarah"),
        EntId::derive("project", "sarah")
    );
    assert_eq!(
        hex(&EntId::derive("project", "sarah").0),
        "34187d34a1f64e2b9bc7ff655654f58161d410c590a86d872ee03fe11be3354a"
    );
    assert_eq!(
        hex(&EntId::derive("tag", "work").0),
        "22f99c5b51d4d671f692387a3674594e2c915506980e241d00aadc2d044b589a"
    );

    // The separator earns its byte: without it these two would hash the same,
    // and a person called `b` would be a persona called nothing.
    assert_ne!(EntId::derive("person", "ab"), EntId::derive("persona", "b"));

    // Non-ASCII keeps its spelling exactly. Unicode folding is locale- and
    // version-dependent, and this value is hashed into the log — a folding rule
    // that shifted on a dependency bump would split every entity whose name
    // contained a character it treated differently.
    assert_ne!(EntId::derive("person", "İ"), EntId::derive("person", "i̇"));
}

#[test]
fn an_entity_rename_encodes_the_same_way_for_ever() {
    use capture_core::body::EntityRenameBody;
    use capture_core::id::EntId;

    let rename = EntityRenameBody {
        nonce: [0x99; 16],
        ent: EntId([0x22; 32]),
        display: "Sarah Chen".to_string(),
    };
    assert_eq!(
        hex(&canonical_of(&rename)),
        "835099999999999999999999999999999999582022222222222222222222222222222222222222222222222222222222222222226a5361726168204368656e"
    );
}

#[test]
fn an_entity_merge_encodes_the_same_way_for_ever() {
    use capture_core::body::EntityMergeBody;
    use capture_core::id::EntId;

    // Two ids and nothing else. A merge is a redirection, so there is no list
    // of notes to reassign in here — which is exactly what makes it survivable
    // in a log that cannot take anything back.
    let merge = EntityMergeBody {
        nonce: [0x99; 16],
        from: EntId([0x22; 32]),
        into: EntId([0x33; 32]),
    };
    assert_eq!(
        hex(&canonical_of(&merge)),
        "8350999999999999999999999999999999995820222222222222222222222222222222222222222222222222222222222222222258203333333333333333333333333333333333333333333333333333333333333333"
    );
}

#[test]
fn an_entity_bind_encodes_the_same_way_for_ever() {
    use capture_core::body::EntityBindBody;
    use capture_core::id::{EntId, EventId};

    let bound = EntityBindBody {
        nonce: [0x99; 16],
        ent: EntId([0x22; 32]),
        note: Some(EventId([0x11; 32])),
    };
    assert_eq!(
        hex(&canonical_of(&bound)),
        "8350999999999999999999999999999999995820222222222222222222222222222222222222222222222222222222222222222258201111111111111111111111111111111111111111111111111111111111111111"
    );

    // Unbinding, and the same elision `ParentBody`'s vector documents: a
    // trailing `None` disappears, so this is a two-element array rather than a
    // three-element one ending in null.
    //
    // Safe for the same specific reason. The meaning is in the kind — an
    // `entity_bind` carrying no note says "backed by nothing", because there is
    // nothing else such an event could say — so it decodes back to `None` and
    // still beats an earlier bind by last-write-wins.
    let unbound = EntityBindBody {
        nonce: [0x99; 16],
        ent: EntId([0x22; 32]),
        note: None,
    };
    let bytes = canonical_of(&unbound);
    assert_eq!(
        hex(&bytes),
        "82509999999999999999999999999999999958202222222222222222222222222222222222222222222222222222222222222222"
    );
    assert_eq!(
        minicbor::decode::<EntityBindBody>(&bytes).unwrap().note,
        None,
        "unbinding did not survive the round trip"
    );
}

#[test]
fn muting_an_entity_encodes_the_same_way_for_ever() {
    use capture_core::id::EntId;

    // A fourth variant on a frozen enum. The three that exist keep their
    // numbers, so every mute already written decodes as it did.
    let mute = MuteBody {
        nonce: [0x99; 16],
        scope: MuteScope::Entity(EntId([0x22; 32])),
        until: Some(1_760_000_000_000),
    };
    assert_eq!(
        hex(&canonical_of(&mute)),
        "835099999999999999999999999999999999820381582022222222222222222222222222222222222222222222222222222222222222221b00000199c82cc000"
    );
}

#[test]
fn an_item_tick_encodes_the_same_way_for_ever() {
    use capture_core::body::ItemBody;

    // With the position and the count, which is what every tick written from
    // now on carries. The count is what makes the position usable at all: a
    // position written against a different number of items cannot be compared,
    // because removing one leaves its slot holding the item below it.
    let ticked = ItemBody {
        nonce: [0x99; 16],
        item: "eggs".to_string(),
        ord: Some(1),
        of: Some(3),
    };
    assert_eq!(
        hex(&canonical_of(&ticked)),
        "84509999999999999999999999999999999964656767730103"
    );

    // Without it — and this is the compatibility property, in bytes.
    //
    // A trailing `None` is elided, so this is a two-element array, which is
    // *exactly* what `ItemBody` encoded to before the position existed. Every
    // tick already written in every archive is still a valid encoding of this
    // type, and decodes with no position rather than failing.
    let legacy = ItemBody {
        nonce: [0x99; 16],
        item: "eggs".to_string(),
        ord: None,
        of: None,
    };
    let bytes = canonical_of(&legacy);
    assert_eq!(
        hex(&bytes),
        "8250999999999999999999999999999999996465676773"
    );

    let back: ItemBody = minicbor::decode(&bytes).unwrap();
    assert_eq!(back.ord, None, "an older tick did not read back");
    assert_eq!(back.item, "eggs", "an older tick lost its words");

    // And the new shape round-trips whole.
    let back: ItemBody = minicbor::decode(&canonical_of(&ticked)).unwrap();
    assert_eq!(back.ord, Some(1));
    assert_eq!(back.of, Some(3));
}
