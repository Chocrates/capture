//! Synthetic event sets, for exercising rules a single device cannot reach.
//!
//! # Why this exists
//!
//! Most of the hard rules in this crate — gap-free sequences, the split between
//! rejected and quarantined events, concurrent edits producing a divergence —
//! only do observable work when *several devices* have written to the same log.
//! There is one device. So without this module those rules would ship untested,
//! and their first real exercise would be against a log already full of events
//! written under them.
//!
//! This builds event sets that look like they came from several devices:
//! consecutive sequence numbers per origin, correct chain links, timestamps
//! consistent with causality, and real signatures. Then it perturbs them in the
//! specific ways a network does.
//!
//! # Deterministic by construction
//!
//! Every generator takes a seed and uses a small hand-rolled generator rather
//! than a random-number library. Two reasons: this crate cannot reach the
//! system entropy source at all, and a failing property test is worthless if
//! the input that failed cannot be reproduced exactly.

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use ed25519_dalek::{Signer, SigningKey};

use crate::{
    Incarnation,
    body::{
        AliasBody, Ambient, CaptureBody, EditBody, OriginClaimBody, PinBody, ResolveBody, TagBody,
        UntagBody,
    },
    envelope::Envelope,
    event::{ENVELOPE_VERSION, Header, Kind},
    hlc::HlcState,
    id::{BodyHash, Dev, EventId},
};

/// A deterministic pseudo-random source.
///
/// xorshift64*, chosen for being ten lines rather than for statistical quality.
/// Nothing here needs unpredictability — only reproducibility.
#[derive(Clone)]
pub struct Rng(u64);

impl Rng {
    /// Seed the generator. Zero is remapped, since xorshift is stuck there.
    pub fn new(seed: u64) -> Self {
        Rng(if seed == 0 {
            0x9E37_79B9_7F4A_7C15
        } else {
            seed
        })
    }

    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// Uniform-ish integer below `n`.
    pub fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            (self.next() % n as u64) as usize
        }
    }

    /// A coin flip weighted `num`/`den`.
    pub fn chance(&mut self, num: u32, den: u32) -> bool {
        (self.next() % den as u64) < num as u64
    }
}

/// One simulated device.
pub struct Origin {
    key: SigningKey,
    inc: Incarnation,
    dev: Dev,
    clock: HlcState,
    seq: u64,
    last: Option<EventId>,
}

impl Origin {
    /// Create a device with a deterministic key.
    pub fn new(n: u8) -> Self {
        let key = SigningKey::from_bytes(&[n.wrapping_add(1); 32]);
        let inc = Incarnation([n; 16]);
        let dev = Dev::derive(&key.verifying_key().to_bytes(), &inc);
        Origin {
            key,
            inc,
            dev,
            clock: HlcState::new(),
            seq: 0,
            last: None,
        }
    }

    /// This device's origin identity.
    pub fn dev(&self) -> Dev {
        self.dev
    }

    /// The public key a verifier needs.
    pub fn pubkey(&self) -> [u8; 32] {
        self.key.verifying_key().to_bytes()
    }

    /// Author one event, maintaining sequence and chain automatically.
    ///
    /// Sequence numbers advance by exactly one and the chain always names the
    /// previous event, so anything this produces is well-formed by
    /// construction. Malformed input is produced by perturbing afterwards,
    /// which keeps "what a correct stream looks like" in one place.
    pub fn author(
        &mut self,
        kind: Kind,
        tgt: Option<EventId>,
        prev: Option<EventId>,
        body: Vec<u8>,
        now: u64,
    ) -> Envelope {
        self.seq += 1;
        let hlc = self.clock.issue(now, self.dev);
        let header = Header {
            v: ENVELOPE_VERSION,
            dev: self.dev,
            seq: self.seq,
            hlc,
            kind,
            tgt,
            prev,
            occ: None,
            chain: self.last,
            bhash: BodyHash::of(&body),
        };
        let sig = self.key.sign(&header.id().0).to_bytes();
        self.last = Some(header.id());
        Envelope {
            header,
            body: Some(body),
            sig,
        }
    }

    /// The same key on a new incarnation: what a restore produces.
    ///
    /// A fresh origin, so its sequence starts again at 1 — which is the whole
    /// reason a restore cannot simply keep authoring under the old one.
    pub fn restored(&self, incarnation: [u8; 16]) -> Origin {
        let inc = Incarnation(incarnation);
        Origin {
            key: self.key.clone(),
            dev: Dev::derive(&self.key.verifying_key().to_bytes(), &inc),
            inc,
            clock: self.clock,
            seq: 0,
            last: None,
        }
    }

    /// Author the self-signed admission that starts a roster.
    pub fn roster_root(&mut self) -> Envelope {
        self.membership(Kind::DEVICE_ADD)
    }

    /// Admit another device: the event a pairing produces.
    pub fn admits(&mut self, other: &Origin) -> Envelope {
        self.admits_as(other, None, None)
    }

    /// Admit another origin as an agent enrolled under `name`.
    pub fn admits_agent(&mut self, other: &Origin, name: &str) -> Envelope {
        self.admits_as(other, Some(crate::body::ROLE_AGENT), Some(name))
    }

    fn admits_as(&mut self, other: &Origin, role: Option<u8>, name: Option<&str>) -> Envelope {
        let body = OriginClaimBody {
            nonce: [0u8; 16],
            pubkey: other.key.verifying_key().to_bytes(),
            incarnation: other.inc.0,
            noise: None,
            role,
            name: name.map(Into::into),
        };
        let mut buf = Vec::new();
        minicbor::encode(&body, &mut buf).expect("encoding into Vec is infallible");
        self.author(Kind::DEVICE_ADD, None, None, buf, 1_700_000_000_000)
    }

    /// Retire another device, naming where each of its origins stops.
    pub fn retires(&mut self, other: &Origin, heads: &[(Dev, u64, EventId)]) -> Envelope {
        self.retires_superseding(other, heads, None)
    }

    /// Reissue a retirement, naming the one it replaces.
    pub fn retires_superseding(
        &mut self,
        other: &Origin,
        heads: &[(Dev, u64, EventId)],
        supersedes: Option<EventId>,
    ) -> Envelope {
        let body = crate::body::RetireBody {
            supersedes,
            nonce: [0u8; 16],
            pubkey: other.key.verifying_key().to_bytes(),
            heads: heads
                .iter()
                .map(|(dev, seq, last)| crate::body::RetiredHead {
                    dev: *dev,
                    seq: *seq,
                    last: *last,
                })
                .collect(),
        };
        let mut buf = Vec::new();
        minicbor::encode(&body, &mut buf).expect("encoding into Vec is infallible");
        self.author(Kind::DEVICE_RETIRE, None, None, buf, 1_700_000_000_000)
    }

    /// Retire this device, by its own hand.
    pub fn retires_self(&mut self) -> Envelope {
        let body = crate::body::RetireBody {
            nonce: [0u8; 16],
            pubkey: self.key.verifying_key().to_bytes(),
            heads: Vec::new(),
            supersedes: None,
        };
        let mut buf = Vec::new();
        minicbor::encode(&body, &mut buf).expect("encoding into Vec is infallible");
        self.author(Kind::DEVICE_RETIRE, None, None, buf, 1_700_000_000_000)
    }

    /// Re-admit this device, co-signed by another.
    ///
    /// `None` co-signs with this device's own key — the forgery the two-signer
    /// rule exists to refuse, and therefore a thing a test has to be able to
    /// build.
    pub fn readmits_self(&mut self, cosigner: Option<&Origin>) -> Envelope {
        let pubkey = self.key.verifying_key().to_bytes();
        let nonce = [0u8; 16];
        let digest = crate::body::readmit_digest(&pubkey, &self.inc.0, &nonce);
        let signer = cosigner.map_or(&self.key, |o| &o.key);
        let sig = ed25519_dalek::Signer::sign(signer, &digest);

        let body = crate::body::ReadmitBody {
            nonce,
            pubkey,
            incarnation: self.inc.0,
            cosigner: signer.verifying_key().to_bytes(),
            cosign: sig.to_bytes(),
        };
        let mut buf = Vec::new();
        minicbor::encode(&body, &mut buf).expect("encoding into Vec is infallible");
        self.author(Kind::DEVICE_READMIT, None, None, buf, 1_700_000_000_000)
    }

    /// Author this origin's claim to author under its key.
    pub fn origin_claim(&mut self) -> Envelope {
        self.membership(Kind::ORIGIN_CLAIM)
    }

    /// Author a rotation of the roster secret.
    pub fn roster_key(&mut self, secret: [u8; 32]) -> Envelope {
        let body = crate::body::RosterKeyBody {
            nonce: [0u8; 16],
            secret,
        };
        let mut buf = Vec::new();
        minicbor::encode(&body, &mut buf).expect("encoding into Vec is infallible");
        self.author(Kind::ROSTER_KEY, None, None, buf, 1_700_000_000_000)
    }

    fn membership(&mut self, kind: Kind) -> Envelope {
        let body = OriginClaimBody {
            nonce: [0u8; 16],
            pubkey: self.key.verifying_key().to_bytes(),
            incarnation: self.inc.0,
            // Deliberately absent. A synthetic origin is not a device that can
            // be dialled, and giving it a Noise key would let a test pass while
            // asserting nothing about how a real one is published.
            noise: None,
            role: None,
            name: None,
        };
        let mut buf = Vec::new();
        minicbor::encode(&body, &mut buf).expect("encoding into Vec is infallible");
        self.author(kind, None, None, buf, 1_700_000_000_000)
    }

    /// Take in a foreign event, so later local events order after it.
    ///
    /// This is what makes generated sets *causally* coherent rather than merely
    /// well-formed: an edit authored after reading someone else's event carries
    /// a timestamp that sorts after it.
    pub fn observe(&mut self, e: &Envelope, now: u64) {
        self.clock.observe(&e.header.hlc, now);
    }
}

/// Encode a body to canonical bytes.
pub fn encode<T: minicbor::Encode<()>>(v: &T) -> Vec<u8> {
    let mut b = Vec::new();
    minicbor::encode(v, &mut b).expect("encoding into Vec is infallible");
    b
}

/// A capture payload with the given text.
pub fn capture_body(nonce: u8, text: &str) -> Vec<u8> {
    encode(&CaptureBody {
        nonce: [nonce; 16],
        text: text.to_string(),
        tz: Some("Europe/London".to_string()),
        ambient: Ambient::default(),
        blob: None,
    })
}

/// An edit payload.
pub fn edit_body(nonce: u8, text: &str) -> Vec<u8> {
    encode(&EditBody {
        nonce: [nonce; 16],
        text: text.to_string(),
    })
}

/// A tag payload.
pub fn tag_body(nonce: u8, tag: &str) -> Vec<u8> {
    encode(&TagBody {
        nonce: [nonce; 16],
        tag: tag.to_string(),
    })
}

/// An untag payload naming the additions it retracts.
pub fn untag_body(nonce: u8, kills: Vec<EventId>) -> Vec<u8> {
    encode(&UntagBody {
        nonce: [nonce; 16],
        kills,
    })
}

/// A pin payload carrying the value, so unpinning is expressible.
pub fn pin_body(nonce: u8, pinned: bool) -> Vec<u8> {
    encode(&PinBody {
        nonce: [nonce; 16],
        pinned,
    })
}

/// An alias payload. `None` clears whatever name the note had.
pub fn alias_body(nonce: u8, alias: Option<&str>) -> Vec<u8> {
    encode(&AliasBody {
        nonce: [nonce; 16],
        alias: alias.map(|a| a.to_string()),
    })
}

/// A dismissal naming the specific versions it clears.
pub fn resolve_body(nonce: u8, dismiss: Vec<EventId>) -> Vec<u8> {
    encode(&ResolveBody {
        nonce: [nonce; 16],
        dismiss,
    })
}

/// A generated set of events, plus the keys needed to verify them.
pub struct Scenario {
    /// Every event, in authoring order.
    pub events: Vec<Envelope>,
    /// Public key per origin, for verification.
    pub keys: Vec<(Dev, [u8; 32])>,
    /// Root capture identifiers, for reference in assertions.
    pub notes: Vec<EventId>,
}

impl Scenario {
    /// The events shuffled into a different order.
    ///
    /// Delivery order is arbitrary in a real network, and the fold is supposed
    /// to be indifferent to it. This is the perturbation that checks that.
    pub fn shuffled(&self, seed: u64) -> Vec<Envelope> {
        let mut rng = Rng::new(seed);
        let mut v = self.events.clone();
        for i in (1..v.len()).rev() {
            v.swap(i, rng.below(i + 1));
        }
        v
    }

    /// Look up an origin's public key.
    pub fn key_for(&self, dev: &Dev) -> Option<[u8; 32]> {
        self.keys.iter().find(|(d, _)| d == dev).map(|(_, k)| *k)
    }
}

/// Build a set of captures, edits and tags across several devices.
///
/// `origins` devices, `notes` notes, and roughly `ops` further operations
/// spread across them. Devices observe each other's events as they go, so
/// sequential edits are genuinely sequential and concurrent ones are genuinely
/// concurrent.
pub fn mixed(seed: u64, origins: usize, notes: usize, ops: usize) -> Scenario {
    let mut rng = Rng::new(seed);
    let mut devs: Vec<Origin> = (0..origins).map(|i| Origin::new(i as u8)).collect();
    let mut events = Vec::new();
    let mut roots: Vec<EventId> = Vec::new();
    let mut heads: Vec<EventId> = Vec::new();
    let mut now = 1_700_000_000_000u64;

    for n in 0..notes {
        let d = rng.below(origins);
        now += 1 + rng.below(50) as u64;
        let body = capture_body(n as u8, "captured text for a synthetic note");
        // `tgt` is left empty on a capture. A capture *is* the note, so its
        // target is its own identifier — but the identifier is computed over
        // `tgt`, so writing it in would change the value being written. The
        // fold reads an empty `tgt` on a capture as the event's own id.
        let e = devs[d].author(Kind::CAPTURE, None, None, body, now);
        roots.push(e.header.id());
        heads.push(e.header.id());
        events.push(e);
    }

    for k in 0..ops {
        if roots.is_empty() {
            break;
        }
        let note = rng.below(roots.len());
        let d = rng.below(origins);
        now += 1 + rng.below(200) as u64;

        // Everything this device has not yet seen, it now sees. That is what
        // makes the resulting edit sequential rather than concurrent.
        let seen: Vec<Envelope> = events.clone();
        for e in &seen {
            if e.header.dev != devs[d].dev {
                devs[d].observe(e, now);
            }
        }

        let e = if rng.chance(1, 3) {
            let body = tag_body(k as u8, if rng.chance(1, 2) { "urgent" } else { "acme" });
            devs[d].author(Kind::TAG, Some(roots[note]), None, body, now)
        } else {
            let body = edit_body(k as u8, "edited text");
            let ev = devs[d].author(Kind::EDIT, Some(roots[note]), Some(heads[note]), body, now);
            heads[note] = ev.header.id();
            ev
        };
        events.push(e);
    }

    Scenario {
        events,
        keys: devs.iter().map(|d| (d.dev(), d.pubkey())).collect(),
        notes: roots,
    }
}

/// A scenario carrying the M2 kinds: promotions and occurrence exceptions.
///
/// Separate from [`mixed`] rather than folded into it, so that every existing
/// test keeps the event counts it was written against. What this exists for is
/// the two properties that matter across kinds — that arrival order does not
/// change the fold, and that an incremental fold matches a full rebuild — and
/// neither means anything for a kind the generator never produces.
///
/// It deliberately generates the awkward combinations: two promotions of
/// differing authority on one note, a cancel and a move naming the same
/// occurrence, and a completion followed by an un-completion.
pub fn typed(seed: u64) -> Scenario {
    let mut rng = Rng::new(seed);
    let mut devs: Vec<Origin> = (0..2).map(|i| Origin::new(i as u8)).collect();
    let mut events = Vec::new();
    let mut roots = Vec::new();
    let mut now = 1_700_000_000_000u64;

    for n in 0..3u8 {
        now += 1 + rng.below(50) as u64;
        let d = rng.below(2);
        let e = devs[d].author(
            Kind::CAPTURE,
            None,
            None,
            capture_body(n, "a synthetic capture with a time in it"),
            now,
        );
        roots.push(e.header.id());
        events.push(e);
    }

    // Everything below is authored after seeing everything above, so these are
    // sequential rather than concurrent — divergence is `concurrent_edit`'s job.
    let observe = |devs: &mut Vec<Origin>, events: &Vec<Envelope>, d: usize, now: u64| {
        let seen = events.clone();
        for e in &seen {
            if e.header.dev != devs[d].dev {
                devs[d].observe(e, now);
            }
        }
    };

    for (i, root) in roots.clone().into_iter().enumerate() {
        // A machine promotion, then a human one over the top of it.
        for (k, pinned) in [(0u8, false), (1u8, true)] {
            now += 1 + rng.below(200) as u64;
            let d = rng.below(2);
            observe(&mut devs, &events, d, now);
            let body = encode(&crate::body::PromoteBody {
                nonce: [k + i as u8; 16],
                as_: crate::body::Promoted::Task(crate::body::TaskFields {
                    done: pinned,
                    due: None,
                    order: None,
                }),
                pinned,
                grammar: if pinned { None } else { Some(1) },
                confidence: if pinned { None } else { Some(85) },
                released: None,
            });
            events.push(devs[d].author(Kind::PROMOTE, Some(root), None, body, now));
        }

        // One occurrence completed, un-completed, cancelled and moved.
        let occ = 1_755_781_200_000i64 + i as i64 * 86_400_000;
        for kind in [
            Kind::OCC_DONE,
            Kind::OCC_UNDONE,
            Kind::OCC_CANCEL,
            Kind::OCC_MOVE,
        ] {
            now += 1 + rng.below(200) as u64;
            let d = rng.below(2);
            observe(&mut devs, &events, d, now);
            let body = if kind == Kind::OCC_MOVE {
                encode(&crate::body::OccMoveBody {
                    nonce: [7u8; 16],
                    occ,
                    start_ms: occ + 3_600_000,
                    end_ms: None,
                })
            } else {
                encode(&crate::body::OccBody {
                    nonce: [8u8; 16],
                    occ,
                })
            };
            events.push(devs[d].author(kind, Some(root), None, body, now));
        }
    }

    Scenario {
        events,
        keys: devs.iter().map(|d| (d.dev(), d.pubkey())).collect(),
        notes: roots,
    }
}

/// Two devices edit one note without seeing each other. Exactly one divergence.
pub fn concurrent_edit(seed: u64) -> Scenario {
    let _ = seed;
    let mut a = Origin::new(0);
    let mut b = Origin::new(1);
    let now = 1_700_000_000_000u64;

    let root = a.author(Kind::CAPTURE, None, None, capture_body(1, "original"), now);
    let root_id = root.header.id();

    // B sees the capture, and only the capture.
    b.observe(&root, now + 1);

    // Both now edit from that same version, neither seeing the other.
    let ea = a.author(
        Kind::EDIT,
        Some(root_id),
        Some(root_id),
        edit_body(2, "A's version"),
        now + 10,
    );
    let eb = b.author(
        Kind::EDIT,
        Some(root_id),
        Some(root_id),
        edit_body(3, "B's version"),
        now + 11,
    );

    Scenario {
        events: alloc::vec![root, ea, eb],
        keys: alloc::vec![(a.dev(), a.pubkey()), (b.dev(), b.pubkey())],
        notes: alloc::vec![root_id],
    }
}

/// One device edits twice; another edits once from the original. One divergence.
///
/// This is the case a naive single-hop comparison misses: neither branch head
/// names the other, and neither shares a predecessor with it, so a rule written
/// as two equality checks reports nothing at all.
pub fn branch_vs_single(seed: u64) -> Scenario {
    let _ = seed;
    let mut a = Origin::new(0);
    let mut b = Origin::new(1);
    let now = 1_700_000_000_000u64;

    let root = a.author(Kind::CAPTURE, None, None, capture_body(1, "original"), now);
    let root_id = root.header.id();

    b.observe(&root, now + 1);

    // A edits twice in sequence.
    let a1 = a.author(
        Kind::EDIT,
        Some(root_id),
        Some(root_id),
        edit_body(2, "A first"),
        now + 10,
    );
    let a2 = a.author(
        Kind::EDIT,
        Some(root_id),
        Some(a1.header.id()),
        edit_body(3, "A second"),
        now + 20,
    );

    // B edits once, from the original, having seen neither of A's edits.
    let b1 = b.author(
        Kind::EDIT,
        Some(root_id),
        Some(root_id),
        edit_body(4, "B only"),
        now + 15,
    );

    Scenario {
        events: alloc::vec![root, a1, a2, b1],
        keys: alloc::vec![(a.dev(), a.pubkey()), (b.dev(), b.pubkey())],
        notes: alloc::vec![root_id],
    }
}

/// A tag added on one device and removed on another, concurrently.
///
/// The removal names only the additions it saw, so an addition it never saw
/// must survive.
pub fn concurrent_tag(seed: u64) -> Scenario {
    let _ = seed;
    let mut a = Origin::new(0);
    let mut b = Origin::new(1);
    let now = 1_700_000_000_000u64;

    let root = a.author(
        Kind::CAPTURE,
        None,
        None,
        capture_body(1, "tagged note"),
        now,
    );
    let root_id = root.header.id();

    // A tags it; B sees both the capture and that tag.
    let t1 = a.author(
        Kind::TAG,
        Some(root_id),
        None,
        tag_body(2, "urgent"),
        now + 5,
    );
    b.observe(&root, now + 6);
    b.observe(&t1, now + 6);

    // B removes the tag it saw. A concurrently adds the same tag again —
    // an addition B's removal cannot possibly have observed.
    let untag = b.author(
        Kind::UNTAG,
        Some(root_id),
        None,
        untag_body(3, alloc::vec![t1.header.id()]),
        now + 10,
    );
    let t2 = a.author(
        Kind::TAG,
        Some(root_id),
        None,
        tag_body(4, "urgent"),
        now + 11,
    );

    Scenario {
        events: alloc::vec![root, t1, untag, t2],
        keys: alloc::vec![(a.dev(), a.pubkey()), (b.dev(), b.pubkey())],
        notes: alloc::vec![root_id],
    }
}

/// A scenario containing an event of a kind this build does not understand.
pub fn unknown_kind(seed: u64) -> Scenario {
    let mut s = mixed(seed, 2, 2, 4);
    let mut d = Origin::new(9);
    let body = encode(&TagBody {
        nonce: [0u8; 16],
        tag: String::from("from-the-future"),
    });
    let e = d.author(Kind(4242), Some(s.notes[0]), None, body, 1_700_000_100_000);
    s.keys.push((d.dev(), d.pubkey()));
    s.events.push(e);
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::envelope::{Verdict, verify_chain};

    #[test]
    fn generated_events_verify() {
        let s = mixed(1, 3, 4, 20);
        assert!(!s.events.is_empty());
        for e in &s.events {
            let k = s.key_for(&e.header.dev).expect("key for every origin");
            assert_eq!(
                e.verify(&k),
                Verdict::Authentic,
                "generated event must verify"
            );
        }
    }

    #[test]
    fn sequences_are_gap_free_per_origin() {
        let s = mixed(7, 3, 5, 30);
        for (dev, _) in &s.keys {
            let mut seqs: Vec<u64> = s
                .events
                .iter()
                .filter(|e| e.header.dev == *dev)
                .map(|e| e.header.seq)
                .collect();
            seqs.sort_unstable();
            for (i, n) in seqs.iter().enumerate() {
                assert_eq!(*n, i as u64 + 1, "origin sequences must have no holes");
            }
        }
    }

    #[test]
    fn chains_are_intact_per_origin() {
        let s = mixed(11, 3, 4, 20);
        for (dev, _) in &s.keys {
            let mut run: Vec<Envelope> = s
                .events
                .iter()
                .filter(|e| e.header.dev == *dev)
                .cloned()
                .collect();
            run.sort_by_key(|e| e.header.seq);
            assert!(verify_chain(&run).is_ok(), "generated chain must be intact");
        }
    }

    #[test]
    fn generation_is_reproducible() {
        let a = mixed(42, 3, 3, 10);
        let b = mixed(42, 3, 3, 10);
        let ids_a: Vec<_> = a.events.iter().map(|e| e.header.id()).collect();
        let ids_b: Vec<_> = b.events.iter().map(|e| e.header.id()).collect();
        assert_eq!(ids_a, ids_b, "a failing seed must be reproducible");
    }

    #[test]
    fn different_seeds_differ() {
        let a = mixed(1, 3, 3, 10);
        let b = mixed(2, 3, 3, 10);
        let ids_a: Vec<_> = a.events.iter().map(|e| e.header.id()).collect();
        let ids_b: Vec<_> = b.events.iter().map(|e| e.header.id()).collect();
        assert_ne!(ids_a, ids_b);
    }

    #[test]
    fn shuffling_preserves_the_set() {
        let s = mixed(3, 2, 3, 8);
        let mut a: Vec<_> = s.events.iter().map(|e| e.header.id()).collect();
        let mut b: Vec<_> = s.shuffled(99).iter().map(|e| e.header.id()).collect();
        a.sort();
        b.sort();
        assert_eq!(a, b, "shuffling must reorder, not alter");
    }

    #[test]
    fn concurrent_edits_share_a_predecessor() {
        let s = concurrent_edit(1);
        let edits: Vec<_> = s
            .events
            .iter()
            .filter(|e| e.header.kind == Kind::EDIT)
            .collect();
        assert_eq!(edits.len(), 2);
        assert_eq!(
            edits[0].header.prev, edits[1].header.prev,
            "both must branch from the same version"
        );
    }

    #[test]
    fn a_branch_head_does_not_name_the_other_branch() {
        // The case a single-hop comparison misses entirely.
        let s = branch_vs_single(1);
        let edits: Vec<_> = s
            .events
            .iter()
            .filter(|e| e.header.kind == Kind::EDIT)
            .collect();
        let a2 = &edits[1];
        let b1 = &edits[2];
        assert_ne!(a2.header.prev, b1.header.prev);
        assert_ne!(a2.header.prev, Some(b1.header.id()));
        assert_ne!(b1.header.prev, Some(a2.header.id()));
    }

    #[test]
    fn causality_shows_up_in_the_timestamps() {
        // B's edit was authored after B took in A's capture, so it must sort
        // after it — that is what makes the set causally coherent.
        let s = concurrent_edit(1);
        let root = &s.events[0];
        for e in s.events.iter().skip(1) {
            assert!(
                e.header.hlc > root.header.hlc,
                "an event authored after reading another must sort after it"
            );
        }
    }

    #[test]
    fn the_unknown_kind_scenario_contains_one() {
        let s = unknown_kind(5);
        assert!(s.events.iter().any(|e| !e.header.kind.is_known()));
        for e in &s.events {
            let k = s.key_for(&e.header.dev).unwrap();
            assert_eq!(
                e.verify(&k),
                Verdict::Authentic,
                "an unknown kind is still authentic — that is the whole point"
            );
        }
    }
}
