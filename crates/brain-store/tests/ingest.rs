//! What arrives from a peer, and what happens to it.
//!
//! These go through `Gate` — the same door a local capture uses. A second
//! ingest path would be a second set of rules about signatures, body hashes,
//! sequence numbers and quarantine, and the second set is the one that is
//! subtly wrong, because it is the one nobody exercises until a peer sends
//! something strange.

use brain_core::envelope::Envelope;
use brain_core::testing;
use brain_store::gate::Gate;
use brain_store::log::Log;
use brain_store::ops::Ops;
use brain_store::projection::Store;
use brain_store::sync::StoreArchive;
use brain_sync::anti_entropy::Archive;

struct Bench {
    dir: std::path::PathBuf,
    log: Log,
    state: Store,
    ops: Ops,
}

fn bench(name: &str) -> Bench {
    let dir = std::env::temp_dir().join(format!("brain-ingest-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    Bench {
        log: Log::open(&dir.join("events.db")).unwrap(),
        state: Store::open(&dir.join("state.db")).unwrap(),
        ops: Ops::open(&dir.join("ops.db")).unwrap(),
        dir,
    }
}

impl Drop for Bench {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.dir).ok();
    }
}

/// A member and a few of its captures.
fn member(seed: u8, n: usize) -> (testing::Origin, Vec<Envelope>) {
    let mut o = testing::Origin::new(seed);
    let mut out = vec![o.roster_root()];
    for i in 0..n {
        out.push(o.author(
            brain_core::Kind::CAPTURE,
            None,
            None,
            testing::capture_body(i as u8, "a note"),
            1_700_000_000_000 + i as u64,
        ));
    }
    (o, out)
}

#[test]
fn an_event_arriving_before_its_predecessor_is_held() {
    // A wire produces this; a local append never can. Folding a stream with a
    // hole would produce state that no other replica agrees with, and
    // rejecting it would leave a gap that a version vector cannot represent —
    // so it is held, at its position, until the one below it arrives.
    let mut b = bench("out-of-order");
    let (_, events) = member(1, 3);

    let mut g = Gate::open(&mut b.log, &mut b.state, &b.ops, [0xEE; 32]).unwrap();
    let mut archive = StoreArchive::new(&mut g, &b.ops, 100);

    assert!(archive.ingest(events[0].clone()), "the root");
    // Skip one: offer the third before the second.
    assert!(
        archive.ingest(events[2].clone()),
        "held rather than refused"
    );
    assert_eq!(
        archive.heads().values().copied().max(),
        Some(1),
        "the head moved past an event that had not arrived"
    );

    assert!(archive.ingest(events[1].clone()));
    assert_eq!(
        archive.heads().values().copied().max(),
        Some(3),
        "the held event was not absorbed once its predecessor arrived"
    );
}

#[test]
fn the_same_event_arriving_twice_changes_nothing() {
    // Duplicates are ordinary: two peers relay the same event, or one
    // re-sends after a dropped connection.
    let mut b = bench("duplicate");
    let (_, events) = member(1, 2);

    let mut g = Gate::open(&mut b.log, &mut b.state, &b.ops, [0xEE; 32]).unwrap();
    let mut archive = StoreArchive::new(&mut g, &b.ops, 100);
    for e in &events {
        archive.ingest(e.clone());
    }
    let before = archive.heads();

    for e in &events {
        assert!(
            archive.ingest(e.clone()),
            "a duplicate was reported as refused"
        );
    }
    assert_eq!(archive.heads(), before);
}

#[test]
fn relaying_confers_no_authority() {
    // Events are relayed — the desktop hands you events that originated on the
    // phone — so transport security proves only "the desktop sent this", never
    // "the phone wrote it". An event whose signature does not check out under
    // the key its claimed origin is entitled to is refused however it arrived.
    let mut b = bench("no-authority");
    let (_, events) = member(1, 2);
    let (_, other) = member(9, 1);

    let mut forged = events[1].clone();
    // Keep the header's claimed origin; take the signature from elsewhere.
    forged.sig = other[0].sig;

    let mut g = Gate::open(&mut b.log, &mut b.state, &b.ops, [0xEE; 32]).unwrap();
    let mut archive = StoreArchive::new(&mut g, &b.ops, 100);
    archive.ingest(events[0].clone());

    assert!(
        !archive.ingest(forged),
        "an event signed by the wrong key was accepted because a peer relayed it"
    );
    assert_eq!(
        b.ops.rejected_total().unwrap(),
        1,
        "the refusal was not recorded, so nothing would ever report it"
    );
}

#[test]
fn a_rejected_event_never_occupies_a_position() {
    // The poisoning §5.3 describes: if an unauthenticated event could claim a
    // sequence position and advance `heads`, the genuine event at that
    // position would collide for ever and be permanently unreachable, while
    // every replica reported the position held.
    let mut b = bench("no-position");
    let (_, events) = member(1, 3);
    let (_, other) = member(9, 1);

    let mut forged = events[2].clone();
    forged.sig = other[0].sig;

    let mut g = Gate::open(&mut b.log, &mut b.state, &b.ops, [0xEE; 32]).unwrap();
    let mut archive = StoreArchive::new(&mut g, &b.ops, 100);
    archive.ingest(events[0].clone());
    archive.ingest(events[1].clone());
    let before = archive.heads();

    assert!(!archive.ingest(forged));
    assert_eq!(
        archive.heads(),
        before,
        "a rejected event advanced the version vector and took a position"
    );

    // And the real one still lands.
    assert!(archive.ingest(events[2].clone()));
    assert_eq!(archive.heads().values().copied().max(), Some(3));
}

#[test]
fn a_range_comes_back_in_sequence_order() {
    // The receiver appends them in the order they arrive, and a gap is what it
    // is trying to avoid.
    let mut b = bench("range-order");
    let (o, events) = member(1, 5);

    let mut g = Gate::open(&mut b.log, &mut b.state, &b.ops, [0xEE; 32]).unwrap();
    let mut archive = StoreArchive::new(&mut g, &b.ops, 100);
    for e in &events {
        archive.ingest(e.clone());
    }

    let dev: String = o.dev().0.iter().map(|x| format!("{x:02x}")).collect();
    let got = archive.range(&dev, 2, 4);
    let seqs: Vec<u64> = got.iter().map(|e| e.header.seq).collect();
    assert_eq!(seqs, vec![2, 3, 4]);
}
