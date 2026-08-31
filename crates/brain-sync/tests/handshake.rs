//! What a handshake admits, and what it refuses.
//!
//! Every one of these runs over `Pair` — two queues and no operating system.
//! A suite that needs sockets gets marked ignored, and then it is not a suite.

use brain_sync::session::{HandshakeError, MAX_PAYLOAD, Session};
use brain_sync::transport::Pair;

/// A device's static keypair, as the roster would hold it.
fn keypair(seed: u8) -> ([u8; 32], [u8; 32]) {
    let secret = [seed; 32];
    let public = x25519_dalek::PublicKey::from(&x25519_dalek::StaticSecret::from(secret));
    (secret, public.to_bytes())
}

/// Run both sides of a handshake against each other.
///
/// The two ends have to be driven together because the pattern alternates and
/// neither side can finish alone — which is exactly what a handshake is, and
/// what a mock that just returned "connected" would fail to model.
fn handshake(
    a_secret: [u8; 32],
    a_expects: [u8; 32],
    b_secret: [u8; 32],
    b_expects: [u8; 32],
) -> (
    Result<Session<brain_sync::transport::End>, HandshakeError>,
    Result<Session<brain_sync::transport::End>, HandshakeError>,
) {
    let pair = Pair::new();
    let (ea, eb) = pair.ends();

    // The initiator writes first, so it is stepped first; each side's messages
    // are already queued for the other by the time it looks.
    let responder = std::thread::spawn(move || Session::accept(eb, &b_secret, &b_expects));
    let initiator = Session::initiate(ea, &a_secret, &a_expects);
    let responder = responder
        .join()
        .expect("the responder thread did not panic");
    (initiator, responder)
}

#[test]
fn two_roster_members_reach_a_session() {
    let (a_sec, a_pub) = keypair(1);
    let (b_sec, b_pub) = keypair(2);
    let (a, b) = handshake(a_sec, b_pub, b_sec, a_pub);
    assert!(a.is_ok(), "the initiator did not finish: {:?}", a.err());
    assert!(b.is_ok(), "the responder did not finish: {:?}", b.err());
}

#[test]
fn what_one_side_encrypts_the_other_reads() {
    let (a_sec, a_pub) = keypair(1);
    let (b_sec, b_pub) = keypair(2);
    let (a, b) = handshake(a_sec, b_pub, b_sec, a_pub);
    let (mut a, mut b) = (a.unwrap(), b.unwrap());

    a.send(b"a note, encrypted").unwrap();
    assert_eq!(
        b.recv().unwrap().as_deref(),
        Some(&b"a note, encrypted"[..])
    );

    b.send(b"and the other way").unwrap();
    assert_eq!(
        a.recv().unwrap().as_deref(),
        Some(&b"and the other way"[..])
    );
}

#[test]
fn a_device_that_is_not_on_the_roster_gets_nothing() {
    // §5.3's threat model: the guest phone on the café wifi. It cannot complete
    // the handshake — not "is rejected afterwards", cannot complete it — because
    // it does not hold a key the other side will mix in.
    let (a_sec, a_pub) = keypair(1);
    let (_b_sec, b_pub) = keypair(2);
    let (stranger_sec, _stranger_pub) = keypair(99);

    // A dials what it believes is B. A stranger answers with its own key.
    let (a, stranger) = handshake(a_sec, b_pub, stranger_sec, a_pub);
    assert!(
        a.is_err(),
        "a session was opened with a device that is not B"
    );
    assert!(stranger.is_err(), "the stranger reached a session");
}

#[test]
fn a_member_dialling_the_wrong_key_is_refused_too() {
    // The other direction, which matters just as much: an impostor cannot get
    // in, and neither can a member reach something that is not who it expects.
    let (a_sec, a_pub) = keypair(1);
    let (b_sec, _b_pub) = keypair(2);
    let (_c_sec, c_pub) = keypair(3);

    let (a, b) = handshake(a_sec, c_pub, b_sec, a_pub);
    assert!(
        a.is_err() && b.is_err(),
        "a mismatched pair reached a session"
    );
}

#[test]
fn nothing_readable_is_on_the_wire() {
    // The absolute §5.2 states, checked rather than trusted: what a session
    // writes is not what it was given. A frame that happened to be plaintext
    // would pass every other test in this file.
    use brain_sync::transport::Transport;

    let (a_sec, a_pub) = keypair(1);
    let (b_sec, b_pub) = keypair(2);

    let pair = Pair::new();
    let (ea, eb) = pair.ends();
    let responder = std::thread::spawn(move || Session::accept(eb, &b_sec, &a_pub));
    let mut a = Session::initiate(ea, &a_sec, &b_pub).unwrap();
    let _b = responder.join().unwrap().unwrap();

    // An extra handle on the same queues, taken *after* the handshake so it
    // cannot race the session for the handshake's own frames — and while both
    // sessions are still alive, since an end going away closes the channel.
    let (_spy_a, mut spy_b) = pair.ends();

    let secret = b"the quick brown fox";
    a.send(secret).unwrap();

    let frame = spy_b
        .recv()
        .unwrap()
        .expect("the session wrote something to the wire");
    assert_ne!(frame, secret, "the payload crossed as-is");
    assert!(
        !frame.windows(secret.len()).any(|w| w == secret),
        "the payload crossed the wire in the clear"
    );
    // And it is longer than the plaintext, because every frame carries an
    // authentication tag — which is what makes tampering detectable rather
    // than merely unlikely.
    assert!(frame.len() > secret.len());
}

#[test]
fn a_payload_too_large_for_one_message_is_refused() {
    // A Noise transport message caps at 65535 including its tag. An earlier
    // draft of the design specified 65536-byte blob chunks, which do not fit at
    // all — so the limit is pinned rather than remembered.
    let (a_sec, a_pub) = keypair(1);
    let (b_sec, b_pub) = keypair(2);
    let (a, b) = handshake(a_sec, b_pub, b_sec, a_pub);
    let (mut a, _b) = (a.unwrap(), b.unwrap());

    assert!(
        a.send(&vec![0u8; MAX_PAYLOAD]).is_ok(),
        "the limit itself must fit"
    );
    assert!(
        a.send(&vec![0u8; MAX_PAYLOAD + 1]).is_err(),
        "a payload that cannot fit in a Noise message was accepted"
    );
}
