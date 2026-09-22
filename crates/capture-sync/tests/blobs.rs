//! Moving bytes between two replicas (§5.6).

use std::collections::HashMap;

use capture_sync::blob::{self, Blobs, CHUNK};
use capture_sync::session::Session;
use capture_sync::transport::Pair;

/// A replica's bytes, in memory.
struct Held {
    /// Complete blobs this replica holds.
    have: HashMap<String, Vec<u8>>,
    /// Blobs it refers to and does not hold, with whatever has arrived.
    want: HashMap<String, Vec<u8>>,
}

impl Held {
    fn with(have: &[(&str, Vec<u8>)], want: &[&str]) -> Held {
        Held {
            have: have.iter().map(|(h, b)| ((*h).into(), b.clone())).collect(),
            want: want.iter().map(|h| ((*h).into(), Vec::new())).collect(),
        }
    }
}

impl Blobs for Held {
    fn wanted(&self) -> Vec<String> {
        let mut names: Vec<String> = self.want.keys().cloned().collect();
        // Ordered, so a test asserts on a sequence rather than on a hash
        // table's mood.
        names.sort();
        names
    }

    fn have_bytes(&self, hash: &str) -> u64 {
        self.want.get(hash).map(|b| b.len() as u64).unwrap_or(0)
    }

    fn chunk(&self, hash: &str, offset: u64) -> Option<(u64, Vec<u8>)> {
        let bytes = self.have.get(hash)?;
        let total = bytes.len() as u64;
        let from = offset.min(total) as usize;
        let to = (from + CHUNK).min(bytes.len());
        Some((total, bytes[from..to].to_vec()))
    }

    fn store(&mut self, hash: &str, _offset: u64, total: u64, data: &[u8]) -> bool {
        let held = self.want.entry(hash.into()).or_default();
        held.extend_from_slice(data);
        if held.len() as u64 >= total {
            let whole = std::mem::take(held);
            self.want.remove(hash);
            self.have.insert(hash.into(), whole);
            return true;
        }
        false
    }
}

/// Run one transfer between two ends, and give both replicas back.
fn transfer(mut asker: Held, holder: Held) -> (Held, Held, blob::Moved) {
    let pair = Pair::new();
    let (ea, eb) = pair.ends();

    // A real session, because these messages have no other kind. The blob
    // plane travels inside the Noise session the events came through, and a
    // test that moved bytes over something else would be testing a path that
    // does not exist.
    let serving = std::thread::spawn(move || {
        let mut session = Session::accept_paired(eb, &[9; 32]).expect("session");
        let served = blob::serve(&mut session, &holder).expect("served");
        (holder, served)
    });

    let mut session = Session::initiate_paired(ea, &[9; 32]).expect("session");
    let moved = blob::fetch(&mut session, &mut asker, &|| false).expect("fetched");

    let (holder, _) = serving.join().expect("no panic");
    (asker, holder, moved)
}

#[test]
fn bytes_arrive_and_are_what_was_asked_for() {
    let recording = vec![0x11u8; CHUNK * 2 + 17];
    let name = "b3:aa";

    let (asker, _, moved) = transfer(
        Held::with(&[], &[name]),
        Held::with(&[(name, recording.clone())], &[]),
    );

    assert_eq!(moved.fetched, 1);
    assert_eq!(moved.bytes, recording.len() as u64);
    assert_eq!(asker.have.get(name), Some(&recording), "byte for byte");
}

#[test]
fn a_transfer_resumes_from_what_is_already_here() {
    // §5.6: an interrupted forty megabyte transfer resumes rather than
    // restarts. On a bad enough connection a replica that started over each
    // round would never finish at all — and this is a phone, on a network it
    // does not control, which is the case the whole design is about.
    let recording = vec![0x22u8; CHUNK * 3];
    let name = "b3:bb";

    let mut asker = Held::with(&[], &[name]);
    asker.want.insert(name.into(), recording[..CHUNK].to_vec());

    let (asker, _, moved) = transfer(asker, Held::with(&[(name, recording.clone())], &[]));

    assert_eq!(asker.have.get(name), Some(&recording), "and it is whole");
    assert_eq!(
        moved.bytes,
        (CHUNK * 2) as u64,
        "only the part that was missing crossed the wire"
    );
}

#[test]
fn a_peer_that_does_not_have_it_says_so() {
    // An evicted blob is the ordinary state of a constrained device, not a
    // fault. The asker should be told at once so it can go to somebody else;
    // §5.6 makes eviction aggressive precisely because it is recoverable, and
    // that is only true if a miss is fast.
    let (asker, _, moved) = transfer(Held::with(&[], &["b3:cc"]), Held::with(&[], &[]));

    assert_eq!(moved.missing, 1);
    assert_eq!(moved.fetched, 0);
    assert!(
        asker.have.is_empty(),
        "nothing was invented to fill the gap"
    );
}

#[test]
fn several_blobs_move_in_one_phase() {
    let one = vec![0x33u8; 100];
    let two = vec![0x44u8; CHUNK + 1];

    let (asker, _, moved) = transfer(
        Held::with(&[], &["b3:d1", "b3:d2"]),
        Held::with(&[("b3:d1", one.clone()), ("b3:d2", two.clone())], &[]),
    );

    assert_eq!(moved.fetched, 2);
    assert_eq!(asker.have.get("b3:d1"), Some(&one));
    assert_eq!(asker.have.get("b3:d2"), Some(&two));
}

#[test]
fn an_empty_blob_is_complete_the_moment_it_starts() {
    // Zero bytes is a legitimate thing to hold, and a loop that waits for
    // progress before deciding it is finished would never finish this one.
    let (asker, _, moved) = transfer(
        Held::with(&[], &["b3:ee"]),
        Held::with(&[("b3:ee", Vec::new())], &[]),
    );

    assert_eq!(moved.fetched, 1);
    assert_eq!(asker.have.get("b3:ee"), Some(&Vec::new()));
}
