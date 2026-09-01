//! Golden vectors for every message on the wire.
//!
//! # Why a protocol message is as frozen as an event body
//!
//! The moment two builds of this program exist in the world, one of them is
//! older and is decoding what the other wrote. There is no schema negotiation
//! and no migration: a peer either reads a frame or does not. That makes these
//! encodings permanent in the same way `capture-core`'s bodies are, and it earns
//! them the same discipline — the bytes are written down before anything sends
//! one, so a change to them is a decision with a diff rather than an accident
//! with a field.
//!
//! What a later version may do is add a field at the end. What it may never do
//! is renumber one, reuse a number, or change what a number means.

use capture_core::EventId;
use capture_sync::message::{Message, PROTOCOL_VERSION, VersionVector};

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
        sealed: None,
        observes: None,
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
                sealed: None,
                observes: None,
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
        sealed: None,
        observes: None,
    };
    match minicbor::decode::<Message>(&encoded(&m)).unwrap() {
        Message::Hello { v, .. } => assert_eq!(v, PROTOCOL_VERSION),
        other => panic!("a hello decoded as {other:?}"),
    }
}

/// The two markers that end a round. They carry nothing, which is the point:
/// a peer that cannot decode the rest of the protocol must still be able to
/// tell that the other side has stopped talking.
#[test]
fn the_end_markers_encode_the_same_way_for_ever() {
    // array(2): the variant number, then its empty field list.
    assert_eq!(hex(&encoded(&Message::Asked)), "820680");
    assert_eq!(hex(&encoded(&Message::Done)), "820780");
}

// ------------------------------------------------------------- pairing ----
//
// These cross before either device is on the other's roster, which makes them
// the only messages a device decodes from something it has not yet admitted.
// A newcomer running a build older than the admitter's has to read the grant
// or it joins a mesh it cannot talk to.

#[test]
fn an_offer_encodes_the_same_way_for_ever() {
    let m = capture_sync::pairing::Offer {
        pubkey: [0x11; 32],
        incarnation: [0x22; 16],
        noise: [0x33; 32],
    };
    let mut b = Vec::new();
    minicbor::encode(&m, &mut b).expect("encoding into a Vec is infallible");
    assert_eq!(
        hex(&b),
        // array(3) · pubkey(32) · incarnation(16) · noise(32)
        "8358201111111111111111111111111111111111111111111111111111111111111111502222222222222222222222222222222258203333333333333333333333333333333333333333333333333333333333333333"
    );
}

#[test]
fn a_grant_encodes_the_same_way_for_ever() {
    let m = capture_sync::pairing::Grant {
        pubkey: [0x44; 32],
        noise: [0x55; 32],
        members: vec![[0x44; 32].into()],
        secret: Some([0x66; 32]),
        incarnation: Some([0x77; 16]),
    };
    let mut b = Vec::new();
    minicbor::encode(&m, &mut b).expect("encoding into a Vec is infallible");
    assert_eq!(
        hex(&b),
        // array(5) · pubkey(32) · noise(32) · members: array(1) of bytes(32)
        // · secret(32) · incarnation(16). The members are byte strings, not
        // arrays of integers. The incarnation is what lets the newcomer admit
        // the admitter back, which is what makes pairing mutual.
        "8558204444444444444444444444444444444444444444444444444444444444444444582055555555555555555555555555555555555555555555555555555555555555558158204444444444444444444444444444444444444444444444444444444444444444582066666666666666666666666666666666666666666666666666666666666666665077777777777777777777777777777777"
    );
}

/// An archive that has never rotated its secret has none to give, and a
/// newcomer must be able to tell that from being given one.
#[test]
fn a_grant_without_a_secret_encodes_the_same_way_for_ever() {
    let m = capture_sync::pairing::Grant {
        pubkey: [0x44; 32],
        noise: [0x55; 32],
        members: vec![],
        secret: None,
        incarnation: None,
    };
    let mut b = Vec::new();
    minicbor::encode(&m, &mut b).expect("encoding into a Vec is infallible");
    assert_eq!(
        hex(&b),
        // array(3), not array(4): minicbor elides a trailing None, so "no
        // secret" is one field shorter rather than a null in the fourth slot.
        // Anything added after `secret` must therefore tolerate its absence.
        "83582044444444444444444444444444444444444444444444444444444444444444445820555555555555555555555555555555555555555555555555555555555555555580"
    );
}

/// A Hello carrying seals, and one without.
///
/// The pair matters: a build that predates sealing must be able to read a
/// Hello from one that does not, and the absence of the field has to be
/// distinguishable from an empty list — "I seal nothing" and "I do not know
/// what sealing is" are the same behaviour but not the same statement.
#[test]
fn a_hello_with_seals_encodes_the_same_way_for_ever() {
    let mut vv = VersionVector::new();
    vv.insert(String::from("aa"), 41822);

    let m = Message::Hello {
        v: PROTOCOL_VERSION,
        vv: vv.clone(),
        partial: false,
        sealed: Some(vec![capture_sync::message::Seal {
            dev: String::from("bb"),
            seq: 9310,
            last: EventId([0x11; 32]),
            roster: [0x22; 32],
        }]),
        observes: None,
    };
    assert_eq!(
        hex(&encoded(&m)),
        // Hello (variant 0) · array(4): v · vv as a map · partial=false ·
        // seals: array(1) of [dev, seq, last(32), roster(32)].
        "82008401a162616119a35ef4818462626219245e5820111111111111111111111111111111111111111111111111111111111111111158202222222222222222222222222222222222222222222222222222222222222222"
    );

    // And the same Hello without: one array element shorter, because minicbor
    // elides a trailing None.
    let plain = Message::Hello {
        v: PROTOCOL_VERSION,
        vv,
        partial: false,
        sealed: None,
        observes: None,
    };
    assert!(encoded(&plain).len() < encoded(&m).len());
}

/// A Hello saying what this device can watch for (§7.5).
///
/// Field 4, added after seals, and the pairing with the test above is the
/// point: an older build reads this frame, takes the four fields it knows,
/// and ignores the fifth. That is the only kind of change this wire permits —
/// append, never renumber — and it is checked rather than asserted in a
/// comment.
#[test]
fn a_hello_saying_what_it_watches_encodes_the_same_way_for_ever() {
    let mut vv = VersionVector::new();
    vv.insert(String::from("aa"), 41822);

    let m = Message::Hello {
        v: PROTOCOL_VERSION,
        vv: vv.clone(),
        partial: false,
        sealed: None,
        observes: Some(vec![String::from("location"), String::from("time")]),
    };
    assert_eq!(
        hex(&encoded(&m)),
        // Hello (variant 0) · array(5): v · vv as a map · partial=false ·
        // sealed=null, which cannot elide because a later field is present ·
        // observes: array(2) of text.
        "82008501a162616119a35ef4f682686c6f636174696f6e6474696d65"
    );

    // Saying nothing is shorter than saying anything, and is what every build
    // that predates the field writes. "I did not say" and "I watch nothing"
    // are the same behaviour and not the same statement — the second is a
    // claim, and only a build that knows the field can make it.
    let silent = Message::Hello {
        v: PROTOCOL_VERSION,
        vv: vv.clone(),
        partial: false,
        sealed: None,
        observes: None,
    };
    assert!(encoded(&silent).len() < encoded(&m).len());

    let nothing = Message::Hello {
        v: PROTOCOL_VERSION,
        vv,
        partial: false,
        sealed: None,
        observes: Some(Vec::new()),
    };
    assert_ne!(
        encoded(&nothing),
        encoded(&silent),
        "an explicit empty list is a statement and an absent field is not"
    );
}

/// Asking for bytes, and being given them (§5.6).
///
/// These travel inside the Noise session like every other message. A separate
/// byte-serving path is the easy mistake the design names by name: it would
/// put a recording in the clear on the local network while the note describing
/// it was encrypted, which is worse than not having the feature.
#[test]
fn wanting_and_sending_a_blob_encode_the_same_way_for_ever() {
    let hash = String::from("b3:9d9d9d9d9d9d9d9d9d9d9d9d9d9d9d9d9d9d9d9d9d9d9d9d9d9d9d9d9d9d9d9d");

    // Asking from a non-zero offset is the ordinary case, not an edge one: an
    // interrupted forty megabyte transfer resumes rather than restarts, and a
    // phone on a bad connection that started over each round would never
    // finish at all.
    let want = Message::Want {
        hash: hash.clone(),
        have_bytes: 32768,
    };
    assert_eq!(
        hex(&encoded(&want)),
        // Want (variant 8) · array(2): text(67) — "b3:" and 64 hex
        // characters, each one a byte because the name travels as text
        // rather than as the 32 bytes it describes · have_bytes.
        "820882784362333a39643964396439643964396439643964396439643964396439643964396439643964396439643964396439643964396439643964396439643964396439643964198000"
    );

    let blob = Message::Blob {
        hash: hash.clone(),
        offset: 32768,
        total: 41822,
        data: vec![0x11, 0x22, 0x33],
    };
    assert_eq!(
        hex(&encoded(&blob)),
        // Blob (variant 9) · array(4): hash · offset · total · data as bytes.
        "820984784362333a3964396439643964396439643964396439643964396439643964396439643964396439643964396439643964396439643964396439643964396439643964396419800019a35e43112233"
    );

    // A miss is stated rather than left to a timeout. A device that has
    // evicted a blob is in the ordinary case and the asker should go to
    // somebody else at once — §5.6 makes eviction aggressive on constrained
    // devices precisely because it is cheap and recoverable, which is only
    // true if a miss is fast.
    let miss = Message::NoBlob { hash };
    assert_eq!(
        hex(&encoded(&miss)),
        "820a81784362333a39643964396439643964396439643964396439643964396439643964396439643964396439643964396439643964396439643964396439643964396439643964"
    );
}

/// A chunk fits in a Noise message, with its framing.
///
/// The number is pinned here rather than remembered. An earlier draft of the
/// design specified 65536-byte chunks, which do not fit at all — the transport
/// would have refused every one of them, and only at the moment somebody first
/// tried to move a recording.
#[test]
fn a_full_chunk_fits_inside_one_noise_message() {
    let blob = Message::Blob {
        hash: String::from("b3:9d9d9d9d9d9d9d9d9d9d9d9d9d9d9d9d9d9d9d9d9d9d9d9d9d9d9d9d9d9d9d9d"),
        offset: 0,
        total: 40 * 1024 * 1024,
        data: vec![0u8; capture_sync::blob::CHUNK],
    };
    assert!(
        encoded(&blob).len() <= capture_sync::session::MAX_PAYLOAD,
        "a chunk plus its framing is {} bytes, and a Noise message holds {}",
        encoded(&blob).len(),
        capture_sync::session::MAX_PAYLOAD,
    );
}
