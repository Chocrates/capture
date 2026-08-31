//! What a six-digit code does and does not admit.

use brain_sync::pairing::{CODE_DIGITS, Grant, Offer, PairingError, admit, code_from, join};
use brain_sync::transport::Pair;

fn offer() -> Offer {
    Offer {
        pubkey: [1; 32],
        incarnation: [2; 16],
        noise: [3; 32],
    }
}

fn grant() -> Grant {
    Grant {
        pubkey: [4; 32],
        noise: [5; 32],
        members: vec![[4; 32].into(), [6; 32].into()],
        secret: Some([7; 32]),
    }
}

/// Run both halves against each other, each on its own thread.
///
/// They have to run together: neither side can finish alone, which is what
/// makes this an exchange rather than a claim one device makes about itself.
type Both = (
    Result<Grant, PairingError>,
    Result<Result<Offer, PairingError>, ()>,
);

fn pair_with(joiner_code: &[u8], admitter_code: &[u8]) -> Both {
    let pair = Pair::new();
    let (ea, eb) = pair.ends();

    let admitter_code = admitter_code.to_vec();
    let admitting = std::thread::spawn(move || admit(eb, &admitter_code, |_| grant()));
    let joined = join(ea, joiner_code, &offer());

    (joined, admitting.join().map_err(|_| ()))
}

#[test]
fn the_right_code_admits_the_device() {
    let (joined, admitted) = pair_with(b"418302", b"418302");

    let received = joined.expect("the joiner did not finish");
    assert_eq!(received.members.len(), 2, "the roster did not arrive");
    assert_eq!(
        received.secret,
        Some([7; 32]),
        "the mesh secret did not arrive"
    );

    let offered = admitted.unwrap().expect("the admitter did not finish");
    assert_eq!(offered.pubkey, [1; 32]);
    assert_eq!(
        offered.noise, [3; 32],
        "without the X25519 key the newcomer is a member nobody can dial"
    );
}

#[test]
fn a_wrong_code_admits_nothing() {
    // The whole point. A device on the network guessing the code learns
    // nothing about the roster, and the admitter learns nothing to write a
    // device_add from.
    let (joined, admitted) = pair_with(b"418302", b"999999");

    assert_eq!(joined, Err(PairingError::Refused));
    assert_eq!(admitted.unwrap(), Err(PairingError::Refused));
}

#[test]
fn a_wrong_code_never_reaches_the_grant() {
    // The admission is written by the callback, so a callback invoked on a
    // failed exchange would be a device_add for whoever was guessing.
    let pair = Pair::new();
    let (ea, eb) = pair.ends();

    let asked = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flag = asked.clone();

    let admitting = std::thread::spawn(move || {
        admit(eb, b"999999", move |_| {
            flag.store(true, std::sync::atomic::Ordering::SeqCst);
            grant()
        })
    });
    let _ = join(ea, b"418302", &offer());
    let _ = admitting.join();

    assert!(
        !asked.load(std::sync::atomic::Ordering::SeqCst),
        "a device_add would have been written for a device that failed the exchange"
    );
}

#[test]
fn the_offer_is_not_on_the_wire() {
    // Everything after the SPAKE2 messages is inside a Noise session. If the
    // offer crossed in the clear, an observer would hold the newcomer's keys
    // and the roster without ever guessing the code.
    let pair = Pair::new();
    let (ea, eb) = pair.ends();

    let admitting = std::thread::spawn(move || admit(eb, b"418302", |_| grant()));
    let _ = join(ea, b"418302", &offer());
    let _ = admitting.join();

    let seen = pair.wire();
    assert!(!seen.is_empty(), "nothing crossed at all");

    let needle = [7u8; 32]; // the mesh secret, from the grant
    assert!(
        !seen.windows(needle.len()).any(|w| w == needle),
        "the roster secret crossed the wire in the clear"
    );
    let key = [1u8; 32]; // the joiner's public key, from the offer
    assert!(
        !seen.windows(key.len()).any(|w| w == key),
        "the offer crossed the wire in the clear"
    );
}

#[test]
fn a_code_is_six_digits() {
    let c = code_from([0; 8]);
    assert_eq!(c.len(), CODE_DIGITS);
    assert!(c.iter().all(|b| b.is_ascii_digit()));
    assert_eq!(c, b"000000", "a leading zero was dropped");

    let c = code_from(u64::MAX.to_be_bytes());
    assert_eq!(c.len(), CODE_DIGITS);
    assert!(c.iter().all(|b| b.is_ascii_digit()));
}

#[test]
fn codes_differ() {
    let a = code_from(1u64.to_be_bytes());
    let b = code_from(2u64.to_be_bytes());
    assert_ne!(a, b);
}
