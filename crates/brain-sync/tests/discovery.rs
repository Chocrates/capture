//! What an observer on the network can and cannot learn.
//!
//! These are the two leaks §5.2 names, tested as claims about an outsider
//! rather than as assertions that a function returns what it returns.

use brain_core::Dev;
use brain_sync::discovery::{Network, backoff, digest, epoch_at, pseudonym, recognise};

const SECRET: [u8; 32] = [7u8; 32];

fn dev(seed: u8) -> Dev {
    Dev([seed; 32])
}

#[test]
fn a_member_recognises_a_peer_in_the_broadcast() {
    let net = Network::of(b"the office wifi");
    let phone = [1u8; 32];
    let laptop = [2u8; 32];

    let advertised = pseudonym(&SECRET, &net, 100, &phone);
    let seen = recognise(&SECRET, &net, 100, &[laptop, phone], &advertised);

    assert_eq!(
        seen,
        Some(phone),
        "a member could not identify its own peer"
    );
}

#[test]
fn an_observer_cannot_correlate_a_device_across_two_networks() {
    let phone = [1u8; 32];
    let cafe = Network::of(b"cafe wifi");
    let office = Network::of(b"the office wifi");

    assert_ne!(
        pseudonym(&SECRET, &cafe, 100, &phone),
        pseudonym(&SECRET, &office, 100, &phone),
        "the same device advertised the same name on two networks, which is \
         the cross-network identifier §5.2 exists to prevent"
    );
}

#[test]
fn an_outsider_holding_a_peers_public_key_still_cannot_find_it() {
    // The public keys are not secret in any strong sense — the threat is a
    // housemate's laptop, not an attacker who has never seen your devices.
    // What the outsider lacks is the roster secret.
    let net = Network::of(b"cafe wifi");
    let phone = [1u8; 32];
    let outsider_guess = [9u8; 32];

    let advertised = pseudonym(&SECRET, &net, 100, &phone);

    assert_eq!(
        recognise(&outsider_guess, &net, 100, &[phone], &advertised),
        None,
        "the pseudonym was recognisable without the roster secret"
    );
}

#[test]
fn a_retired_device_loses_the_ability_to_follow_the_mesh() {
    // The whole reason the secret is minted rather than derived from the
    // member set: after rotation, the key the retired device still holds
    // recognises nothing.
    let net = Network::of(b"cafe wifi");
    let phone = [1u8; 32];
    let rotated = [8u8; 32];

    let advertised = pseudonym(&rotated, &net, 100, &phone);

    assert_eq!(
        recognise(&SECRET, &net, 100, &[phone], &advertised),
        None,
        "a device holding the old roster secret could still follow its former peers"
    );
}

#[test]
fn the_digest_does_not_report_capture_activity() {
    let net = Network::of(b"home");
    let heads = [(dev(1), 400), (dev(2), 12)];

    // Nothing was captured. The advertised digest changes anyway, so an
    // observer watching it change cannot conclude that anything was.
    assert_ne!(
        digest(&SECRET, &net, 100, &heads),
        digest(&SECRET, &net, 101, &heads),
        "the digest stood still across a rotation, so any change to it would \
         mean an append — which is the activity oracle in §5.2"
    );
}

#[test]
fn the_digest_cannot_be_computed_without_the_secret() {
    let net = Network::of(b"home");
    let heads = [(dev(1), 400)];
    let more = [(dev(1), 401)];

    // An observer who could compute the digest from a guessed vector could
    // binary-search the capture count. Keying is what prevents it.
    assert_ne!(
        digest(&SECRET, &net, 100, &heads),
        digest(&[0u8; 32], &net, 100, &heads)
    );
    assert_ne!(
        digest(&SECRET, &net, 100, &heads),
        digest(&SECRET, &net, 100, &more)
    );
}

#[test]
fn a_member_can_tell_there_is_nothing_to_fetch() {
    // The point of advertising the digest at all: skip the connection.
    let net = Network::of(b"home");
    let heads = [(dev(1), 400), (dev(2), 12)];
    let same_heads_other_order = [(dev(2), 12), (dev(1), 400)];

    assert_eq!(
        digest(&SECRET, &net, 100, &heads),
        digest(&SECRET, &net, 100, &same_heads_other_order),
        "two replicas holding the same events disagreed about the digest, so \
         they would sync forever"
    );
}

#[test]
fn recognition_survives_the_rotation_boundary() {
    // A device advertising under epoch 100 is browsed by a peer whose clock
    // has already turned over. Without accepting the previous epoch, members
    // stop seeing each other for ten minutes out of every ten.
    let net = Network::of(b"home");
    let phone = [1u8; 32];
    let advertised = pseudonym(&SECRET, &net, 100, &phone);

    assert_eq!(
        recognise(&SECRET, &net, 101, &[phone], &advertised),
        Some(phone)
    );
    assert_eq!(
        recognise(&SECRET, &net, 102, &[phone], &advertised),
        None,
        "a stale advertisement was still accepted two epochs on"
    );
}

#[test]
fn epochs_advance_with_the_clock() {
    assert_eq!(epoch_at(0), 0);
    assert_eq!(epoch_at(599), 0);
    assert_eq!(epoch_at(600), 1);
}

#[test]
fn backoff_grows_and_is_bounded() {
    let none = 0;
    assert!(backoff(0, none) < backoff(4, none), "backoff did not grow");
    assert!(
        backoff(30, none) <= 375,
        "backoff exceeded its ceiling, so a peer that is simply switched off \
         would be dialled at ever-widening intervals forever"
    );
    // No overflow at an attempt count a device left running for weeks reaches.
    let _ = backoff(u32::MAX, u64::MAX);
}

#[test]
fn backoff_spreads_retries_out() {
    let base = backoff(5, 0);
    let spread: std::collections::BTreeSet<u64> = (0..64).map(|j| backoff(5, j)).collect();

    assert!(
        spread.len() > 1,
        "every device would retry in the same instant"
    );
    assert!(
        spread.iter().all(|&d| d >= base),
        "jitter pulled a retry earlier rather than spreading it"
    );
}

// ------------------------------------------------ what to do with a sighting ----

use brain_sync::discovery::{Sighting, consider, decode_value, encode_value};

fn sighting(pseudonym: [u8; 8], digest: [u8; 8]) -> Sighting {
    Sighting {
        pseudonym,
        digest,
        addr: "192.168.1.40:9999".into(),
    }
}

#[test]
fn a_member_is_picked_out_of_the_noise() {
    let net = Network::of(b"home");
    let phone = [1u8; 32];
    let s = sighting(pseudonym(&SECRET, &net, 100, &phone), [3; 8]);

    let seen = consider(&SECRET, &net, 100, &[phone], &s).expect("a peer");
    assert_eq!(seen.peer, phone);
    assert_eq!(seen.addr, "192.168.1.40:9999");
}

#[test]
fn a_stranger_on_the_network_is_not_dialled() {
    // The ordinary case on any real network: a printer, a media player, a
    // housemate's laptop. Proximity on the LAN is not membership, and the
    // handshake would refuse it — this only avoids opening the connection to
    // find that out.
    let net = Network::of(b"home");
    let s = sighting([0xAA; 8], [3; 8]);

    assert_eq!(consider(&SECRET, &net, 100, &[[1u8; 32]], &s), None);
}

#[test]
fn advertised_values_survive_the_txt_record() {
    let v = [0x00, 0x01, 0x7f, 0x80, 0xfe, 0xff, 0x10, 0xab];
    assert_eq!(decode_value(&encode_value(&v)), Some(v));
}

#[test]
fn a_txt_record_from_something_else_is_not_a_peer() {
    // Every other service on the network writes whatever it likes here.
    assert_eq!(decode_value(""), None);
    assert_eq!(decode_value("nope"), None);
    assert_eq!(decode_value("zzzzzzzzzzzzzzzz"), None, "not hex");
    assert_eq!(decode_value("00112233445566"), None, "too short");
    assert_eq!(decode_value("001122334455667788"), None, "too long");
}
