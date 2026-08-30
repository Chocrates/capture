//! Golden vectors for every message on the wire.
//!
//! # Why a protocol message is as frozen as an event body
//!
//! The moment two builds of this program exist in the world, one of them is
//! older and is decoding what the other wrote. There is no schema negotiation
//! and no migration: a peer either reads a frame or does not. That makes these
//! encodings permanent in the same way `brain-core`'s bodies are, and it earns
//! them the same discipline — the bytes are written down before anything sends
//! one, so a change to them is a decision with a diff rather than an accident
//! with a field.
//!
//! What a later version may do is add a field at the end. What it may never do
//! is renumber one, reuse a number, or change what a number means.

use brain_core::EventId;
use brain_sync::message::{Message, PROTOCOL_VERSION, VersionVector};

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn encoded(m: &Message) -> Vec<u8> {
    let mut b = Vec::new();
    minicbor::encode(m, &mut b).expect("encoding into a Vec is infallible");
    b
}

#[test]
fn hello_encodes_the_same_way_for_ever() {
    let mut vv = VersionVector::new();
    vv.insert("aa".into(), 41822);
    vv.insert("bb".into(), 9310);

    let m = Message::Hello {
        v: PROTOCOL_VERSION,
        vv,
        partial: false,
    };
    assert_eq!(
        hex(&encoded(&m)),
        // 82 array(2) · 00 hello · 83 array(3) · 01 version · a2 map(2) ·
        // "aa" 41822 · "bb" 9310 · f4 partial=false
        "82008301a262616119a35e62626219245ef4"
    );
}

#[test]
fn range_encodes_the_same_way_for_ever() {
    let m = Message::Range {
        dev: "aa".into(),
        from: 41501,
        to: 41822,
    };
    assert_eq!(hex(&encoded(&m)), "82018362616119a21d19a35e");
}

#[test]
fn an_acknowledgement_encodes_the_same_way_for_ever() {
    let m = Message::RedactAck {
        id: EventId([0x11; 32]),
    };
    assert_eq!(
        hex(&encoded(&m)),
        "8204815820111111111111111111111111111111111111111111111111111111111111\
1111"
    );
}

#[test]
fn a_request_for_a_note_encodes_the_same_way_for_ever() {
    let m = Message::WantTgt {
        tgt: EventId([0x22; 32]),
    };
    assert_eq!(
        hex(&encoded(&m)),
        "8205815820222222222222222222222222222222222222222222222222222222222222\
2222"
    );
}

#[test]
fn the_variant_numbers_are_the_ones_recorded() {
    // The tag is the first thing a peer reads and the last thing that may
    // change. Renumbering one would make an older build act on a message it
    // has misidentified — which is worse than failing to read it, because it
    // would succeed.
    for (m, tag) in [
        (
            Message::Hello {
                v: PROTOCOL_VERSION,
                vv: VersionVector::new(),
                partial: false,
            },
            0u8,
        ),
        (
            Message::Range {
                dev: "aa".into(),
                from: 1,
                to: 2,
            },
            1,
        ),
        (
            Message::RedactAck {
                id: EventId([0; 32]),
            },
            4,
        ),
        (
            Message::WantTgt {
                tgt: EventId([0; 32]),
            },
            5,
        ),
    ] {
        let bytes = encoded(&m);
        assert_eq!(bytes[1], tag, "{m:?} is no longer variant {tag}");
    }
}

#[test]
fn the_protocol_version_is_on_the_wire() {
    // A wire with no version has no way to gain one. Nothing negotiates on it
    // yet — there is one version — but the first incompatible change needs
    // somewhere to be expressed, and that has to exist before the change does.
    assert_eq!(PROTOCOL_VERSION, 1);
    let m = Message::Hello {
        v: PROTOCOL_VERSION,
        vv: VersionVector::new(),
        partial: false,
    };
    match minicbor::decode::<Message>(&encoded(&m)).unwrap() {
        Message::Hello { v, .. } => assert_eq!(v, PROTOCOL_VERSION),
        other => panic!("a hello decoded as {other:?}"),
    }
}
