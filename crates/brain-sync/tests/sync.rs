//! Two replicas, one wire, and whether they end up holding the same events.
//!
//! The archive here is a map, not a store. What is being tested is the
//! protocol's arithmetic — who asks for what, and when each side stops talking
//! — and putting a real store behind it would make a protocol bug look like a
//! store bug.

use std::collections::BTreeMap;

use brain_core::envelope::Envelope;
use brain_core::testing;
use brain_sync::anti_entropy::{Archive, Exchanged, exchange};
use brain_sync::message::VersionVector;
use brain_sync::session::Session;
use brain_sync::transport::Pair;

#[derive(Default)]
struct Replica {
    /// Origin, in hex, to that origin's events by sequence number.
    held: BTreeMap<String, BTreeMap<u64, Envelope>>,
    /// Events this replica refuses, by id, to model ingest rejecting one.
    refuse: Vec<[u8; 32]>,
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

impl Replica {
    fn with(events: &[Envelope]) -> Self {
        let mut r = Self::default();
        for e in events {
            r.put(e.clone());
        }
        r
    }

    fn put(&mut self, e: Envelope) {
        self.held
            .entry(hex(&e.header.dev.0))
            .or_default()
            .insert(e.header.seq, e);
    }

    fn count(&self) -> usize {
        self.held.values().map(|m| m.len()).sum()
    }
}

impl Archive for Replica {
    fn heads(&self) -> VersionVector {
        self.held
            .iter()
            .filter_map(|(dev, events)| events.keys().last().map(|&h| (dev.clone(), h)))
            .collect()
    }

    fn range(&self, dev: &str, from: u64, to: u64) -> Vec<Envelope> {
        self.held
            .get(dev)
            .map(|events| events.range(from..=to).map(|(_, e)| e.clone()).collect())
            .unwrap_or_default()
    }

    fn ingest(&mut self, e: Envelope) -> bool {
        if self.refuse.contains(&e.header.id().0) {
            return false;
        }
        self.put(e);
        true
    }
}

/// Run both halves of one exchange, each on its own thread.
fn sync(a: &mut Replica, b: &mut Replica) -> (Exchanged, Exchanged) {
    let pair = Pair::new();
    let (ea, eb) = pair.ends();

    std::thread::scope(|s| {
        let right = s.spawn(|| {
            let mut session = Session::accept_paired(eb, &[9; 32]).expect("session");
            let out = exchange(&mut session, b).expect("the responder finished");
            (out, session)
        });
        let mut session = Session::initiate_paired(ea, &[9; 32]).expect("session");
        let left = exchange(&mut session, a).expect("the initiator finished");
        let (right, _keep) = right.join().expect("no panic");
        // Both sessions stay alive until here: an end going away closes the
        // channel, and dropping one early would look like a protocol hang.
        (left, right)
    })
}

/// A few events from one origin.
fn origin(seed: u8, n: usize) -> (testing::Origin, Vec<Envelope>) {
    let mut o = testing::Origin::new(seed);
    let mut out = vec![o.roster_root()];
    for i in 0..n {
        out.push(o.author(
            brain_core::event::Kind::CAPTURE,
            None,
            None,
            testing::capture_body(i as u8, "a note"),
            1_700_000_000_000 + i as u64,
        ));
    }
    (o, out)
}

#[test]
fn a_replica_that_is_behind_catches_up() {
    let (_, events) = origin(1, 5);
    let mut ahead = Replica::with(&events);
    let mut behind = Replica::with(&events[..2]);

    let (theirs, mine) = sync(&mut behind, &mut ahead);

    assert_eq!(behind.count(), events.len(), "the replica did not catch up");
    assert_eq!(theirs.received, 4);
    assert_eq!(mine.sent, 4);
    assert_eq!(
        mine.received, 0,
        "the replica that was ahead took in events it had"
    );
}

#[test]
fn two_replicas_in_agreement_exchange_nothing() {
    let (_, events) = origin(1, 4);
    let mut a = Replica::with(&events);
    let mut b = Replica::with(&events);

    let (left, right) = sync(&mut a, &mut b);

    assert_eq!(left, Exchanged::default());
    assert_eq!(right, Exchanged::default());
}

#[test]
fn each_side_gives_the_other_what_it_lacks() {
    // The case a push-only protocol gets wrong: both are behind, in different
    // origins, and one round has to settle both directions.
    let (_, from_a) = origin(1, 4);
    let (_, from_b) = origin(2, 3);

    let mut a = Replica::with(&from_a);
    let mut b = Replica::with(&from_b);

    sync(&mut a, &mut b);

    assert_eq!(a.count(), from_a.len() + from_b.len());
    assert_eq!(b.count(), from_a.len() + from_b.len());
    assert_eq!(
        a.heads(),
        b.heads(),
        "the two replicas disagree about what exists"
    );
}

#[test]
fn a_device_receives_a_third_devices_events_from_a_relay() {
    // The desktop was off while the phone captured, and has never met it. It
    // learns the phone's events from the laptop, which authored none of them.
    // "No device is required to be online" is what makes this serverless
    // rather than one machine quietly being the server.
    let (_, laptop) = origin(1, 2);
    let (_, phone) = origin(2, 4);

    let mut relay = Replica::with(&laptop);
    for e in &phone {
        relay.put(e.clone());
    }
    let mut desktop = Replica::with(&laptop);

    sync(&mut desktop, &mut relay);

    assert_eq!(
        desktop.count(),
        laptop.len() + phone.len(),
        "the desktop did not accept the phone's events from the laptop"
    );
}

#[test]
fn a_refused_event_does_not_stop_the_ones_behind_it() {
    // A peer handing over one event that fails verification is not a reason to
    // abandon the rest — that would let a single bad event deny an entire
    // origin, which is a cheaper attack than forging one.
    let (_, events) = origin(1, 5);
    let mut ahead = Replica::with(&events);
    let mut behind = Replica::with(&events[..1]);
    behind.refuse.push(events[2].header.id().0);

    let (mine, _) = sync(&mut behind, &mut ahead);

    assert_eq!(mine.refused, 1);
    assert_eq!(mine.received, events.len() - 2);
    assert_eq!(behind.count(), events.len() - 1);
}

#[test]
fn a_replica_starting_from_nothing_gets_everything() {
    // A newly paired device. It holds no origins at all, so every range starts
    // at 1 — and if it did not, it would be permanently short of events it
    // could never discover it was missing.
    let (_, events) = origin(1, 6);
    let mut fresh = Replica::default();
    let mut full = Replica::with(&events);

    sync(&mut fresh, &mut full);

    assert_eq!(fresh.count(), events.len());
    assert_eq!(fresh.heads(), full.heads());
}
