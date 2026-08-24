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
    body::{Ambient, CaptureBody},
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
