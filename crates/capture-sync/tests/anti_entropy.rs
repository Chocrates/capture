//! What two replicas ask each other for.

use capture_sync::anti_entropy::{FANOUT, select, wanted};
use capture_sync::message::{Message, VersionVector};

fn vv(pairs: &[(&str, u64)]) -> VersionVector {
    pairs.iter().map(|(d, n)| ((*d).into(), *n)).collect()
}

fn range(m: &Message) -> (&str, u64, u64) {
    match m {
        Message::Range { dev, from, to } => (dev.as_str(), *from, *to),
        other => panic!("expected a range, got {other:?}"),
    }
}

#[test]
fn two_replicas_holding_the_same_events_ask_for_nothing() {
    let a = vv(&[("aa", 40), ("bb", 12)]);
    assert!(wanted(&a, &a).is_empty());
}

#[test]
fn a_replica_asks_for_exactly_what_it_lacks() {
    let mine = vv(&[("aa", 40), ("bb", 12)]);
    let theirs = vv(&[("aa", 43), ("bb", 12)]);

    let asks = wanted(&mine, &theirs);
    assert_eq!(asks.len(), 1, "asked about an origin it is level on");
    assert_eq!(range(&asks[0]), ("aa", 41, 43));
}

#[test]
fn an_origin_never_heard_of_is_asked_for_from_the_beginning() {
    // A device paired into an existing archive holds nothing at all. Starting
    // anywhere but 1 would leave it permanently short of events it can never
    // discover it is missing.
    let asks = wanted(&vv(&[]), &vv(&[("aa", 3)]));
    assert_eq!(range(&asks[0]), ("aa", 1, 3));
}

#[test]
fn a_range_never_starts_past_what_is_held() {
    // A version vector cannot represent a hole. Asking for anything but the
    // successor produces one, and a replica with a hole re-requests the same
    // range from every peer for ever — the pathology §1.1 names.
    for held in 0..50u64 {
        let mine = vv(&[("aa", held)]);
        let asks = wanted(&mine, &vv(&[("aa", 100)]));
        assert_eq!(range(&asks[0]).1, held + 1);
    }
}

#[test]
fn being_ahead_is_not_a_reason_to_ask() {
    // The peer works out its own side. A replica that asked for what it
    // already had would re-fetch the archive on every connection.
    let mine = vv(&[("aa", 90)]);
    let theirs = vv(&[("aa", 40)]);
    assert!(wanted(&mine, &theirs).is_empty());
    assert!(
        !wanted(&theirs, &mine).is_empty(),
        "the peer is behind and needs telling"
    );
}

#[test]
fn events_are_fetched_from_whoever_has_them() {
    // The desktop was off while the phone captured. It learns the phone's
    // events from the laptop, which authored none of them. This is what makes
    // "no device is required to be online" true rather than aspirational.
    let desktop = vv(&[("laptop", 5)]);
    let laptop = vv(&[("laptop", 5), ("phone", 30)]);

    let asks = wanted(&desktop, &laptop);
    assert_eq!(asks.len(), 1);
    assert_eq!(
        range(&asks[0]),
        ("phone", 1, 30),
        "the desktop would not accept the phone's events from the laptop"
    );
}

#[test]
fn each_side_asks_for_its_own_half() {
    // No negotiation round: both diffs are computed from the same pair of
    // vectors, and neither side has to agree to anything.
    let a = vv(&[("aa", 40), ("bb", 3)]);
    let b = vv(&[("aa", 12), ("bb", 30)]);

    assert_eq!(range(&wanted(&a, &b)[0]), ("bb", 4, 30));
    assert_eq!(range(&wanted(&b, &a)[0]), ("aa", 13, 40));
}

#[test]
fn selection_starves_nobody() {
    // With more peers than fanout, every peer must come up eventually — a
    // device that is never selected is a device that never syncs, and nothing
    // would report it.
    let peers: Vec<[u8; 32]> = (1..=5u8).map(|i| [i; 32]).collect();
    let mut seen = std::collections::BTreeSet::new();
    for round in 0..5 {
        for p in select(&peers, round, FANOUT) {
            seen.insert(p);
        }
    }
    assert_eq!(seen.len(), peers.len());
}

#[test]
fn selection_at_two_devices_is_the_other_one() {
    let peers = vec![[1u8; 32]];
    assert_eq!(select(&peers, 0, FANOUT), peers);
    assert_eq!(select(&peers, 7, FANOUT), peers, "and stays so every round");
}

#[test]
fn selection_of_nobody_is_not_a_panic() {
    assert!(select(&[], 3, FANOUT).is_empty());
}

#[test]
fn selection_does_not_repeat_a_peer_within_a_round() {
    // Dialling the same device twice in one round wastes the fanout that
    // exists to spread events, and looks like coverage that is not there.
    let peers: Vec<[u8; 32]> = (1..=4u8).map(|i| [i; 32]).collect();
    for round in 0..8 {
        let picked = select(&peers, round, FANOUT);
        let unique: std::collections::BTreeSet<_> = picked.iter().collect();
        assert_eq!(
            unique.len(),
            picked.len(),
            "round {round} picked a peer twice"
        );
    }
}
