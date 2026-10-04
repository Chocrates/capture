//! A process that does real work to a real store, and expects to be killed.
//!
//! Driven by `tests/crash.rs`. It is a separate binary rather than a thread
//! because the point is to remove the process without letting it clean up:
//! within one process there is no way to stop destructors, buffered writes and
//! open transactions from unwinding tidily, and tidy shutdown is the one thing
//! this must never do.
//!
//! Everything here is deterministic given the seed, so the supervisor knows
//! exactly what work was attempted without the victim having to report it —
//! which matters, because a victim that reported its progress would need to
//! flush that report, and a crash could then be masked by an unflushed line.

use std::path::{Path, PathBuf};

use capture_core::{
    Dev, Incarnation,
    body::CaptureBody,
    envelope::Envelope,
    event::{ENVELOPE_VERSION, Header, Kind},
    fold,
    hlc::HlcState,
    id::BodyHash,
};
use capture_store::{
    Result, drain,
    log::Log,
    ops::Ops,
    projection::{self, Store},
    purge, spool,
};
use ed25519_dalek::{Signer, SigningKey};

fn main() {
    let mut args = std::env::args().skip(1);
    let mode = args.next().unwrap_or_default();
    let root = PathBuf::from(args.next().expect("a store root"));

    let r = match mode.as_str() {
        "append" => append(&root),
        "drain" => drain_loop(&root),
        "rebuild" => rebuild_loop(&root),
        "redact" => redact_loop(&root),
        other => {
            eprintln!("unknown mode {other}");
            std::process::exit(2);
        }
    };
    if let Err(e) = r {
        eprintln!("victim stopped: {e}");
        std::process::exit(1);
    }
}

/// The deterministic capture at position `n`.
///
/// The position is written into the nonce rather than drawn from entropy, so
/// the supervisor can read an index back out of any payload it finds, wherever
/// it finds it. That is what lets it check for a *gap*: a missing index between
/// two present ones is a capture that was accepted and then lost, and no
/// inspection of what happens to survive can detect that on its own.
///
/// Real captures use real entropy. The property under test — that a body names
/// itself uniquely — is unaffected by where the bytes come from.
fn payload(n: u64) -> Vec<u8> {
    let mut nonce = [0u8; 16];
    nonce[..8].copy_from_slice(&n.to_le_bytes());
    let body = CaptureBody {
        nonce,
        text: format!("capture {n}"),
        tz: None,
        ambient: Default::default(),
        blob: None,
    };
    let mut b = Vec::new();
    minicbor::encode(&body, &mut b).expect("encoding into Vec is infallible");
    b
}

/// A device with a fixed key, so a restarted victim is the same origin.
struct Author {
    key: SigningKey,
    dev: Dev,
    clock: HlcState,
    seq: u64,
    last: Option<capture_core::EventId>,
}

impl Author {
    fn new(log: &Log) -> Result<Self> {
        let key = SigningKey::from_bytes(&[7u8; 32]);
        let inc = Incarnation([11u8; 16]);
        let dev = Dev::derive(&key.verifying_key().to_bytes(), &inc);

        // Resume where the last run stopped. A restarted victim that began
        // again at sequence 1 would collide with its own history and the
        // harness would be testing the collision path instead of the crash.
        let heads = log.heads()?;
        let seq = heads.get(&dev_hex(&dev)).copied().unwrap_or(0);
        let last = log
            .all()?
            .into_iter()
            .filter(|e| e.header.dev == dev)
            .max_by_key(|e| e.header.seq)
            .map(|e| e.header.id());

        Ok(Author {
            key,
            dev,
            clock: HlcState::new(),
            seq,
            last,
        })
    }

    /// Sign a header that was adjusted after authoring.
    fn sign(&self, header: &Header) -> [u8; 64] {
        self.key.sign(&header.id().0).to_bytes()
    }

    fn author(&mut self, body: Vec<u8>, now: u64) -> Envelope {
        self.seq += 1;
        let hlc = self.clock.issue(now, self.dev);
        let header = Header {
            v: ENVELOPE_VERSION,
            dev: self.dev,
            seq: self.seq,
            hlc,
            kind: Kind::CAPTURE,
            tgt: None,
            prev: None,
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
}

/// Record that a capture was acknowledged.
///
/// Appended and flushed one line at a time. Buffering would let a crash swallow
/// the very acknowledgements the supervisor is about to check, turning lost
/// data into a clean run.
fn acknowledge(root: &Path, n: u64) -> Result<()> {
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(root.join("acknowledged"))?;
    writeln!(f, "{n}")?;
    f.sync_all()?;
    Ok(())
}

/// Read the index back out of a payload written by [`payload`].
fn index_of(body: &[u8]) -> Option<u64> {
    let b: CaptureBody = minicbor::decode(body).ok()?;
    Some(u64::from_le_bytes(b.nonce[..8].try_into().ok()?))
}

/// The largest capture index the store knows about, committed or spooled.
fn highest_index(log: &Log, spool_path: &Path) -> Result<u64> {
    let mut max = 0;
    for e in log.all()? {
        if let Some(body) = e.body.as_deref() {
            max = max.max(index_of(body).unwrap_or(0));
        }
    }
    for rec in spool::read_all(spool_path)? {
        max = max.max(index_of(&rec.payload).unwrap_or(0));
    }
    Ok(max)
}

fn dev_hex(d: &Dev) -> String {
    d.0.iter().map(|b| format!("{b:02x}")).collect()
}

/// Base timestamp. Fixed, so a restart does not jump the clock past its
/// tolerance and refuse to issue.
const BASE_MS: u64 = 1_700_000_000_000;

/// Append events as fast as the disk allows, until killed.
fn append(root: &Path) -> Result<()> {
    let mut log = Log::open(&root.join("events.db"))?;
    let mut a = Author::new(&log)?;
    let mut n = a.seq;
    loop {
        n += 1;
        let e = a.author(payload(n), BASE_MS + n);
        log.append(&e)?;
    }
}

/// Spool captures and drain them, until killed.
///
/// Deliberately interleaved rather than batched: the interesting kills land
/// between committing a record and clearing the spool, which is a window that
/// only exists while a drain is running.
fn drain_loop(root: &Path) -> Result<()> {
    let spool_path = root.join("spool");
    spool::preallocate(&spool_path)?;
    let mut log = Log::open(&root.join("events.db"))?;
    let mut a = Author::new(&log)?;

    // Continue from the highest index anywhere in the store, counting each
    // index once however many places it appears. Counting rows instead would
    // double-count a record that is both committed and still spooled — the
    // ordinary result of a crash — and skip indices, which is
    // indistinguishable from the data loss this is trying to detect.
    let mut n = highest_index(&log, &spool_path)?;
    loop {
        for _ in 0..4 {
            n += 1;
            spool::push(&spool_path, &payload(n))?;
            // `push` returning is the acknowledgement: at that point the
            // interface would have told the user the capture is safe. Recording
            // it here, durably and outside the store, gives the supervisor an
            // oracle it cannot get by inspecting the store afterwards — a
            // capture that was lost leaves nothing behind to inspect, and the
            // records that remain all look perfectly well-formed.
            acknowledge(root, n)?;
        }
        drain::drain(&spool_path, &mut log, |p| {
            Ok(a.author(p.to_vec(), BASE_MS + n))
        })?;
    }
}

/// Capture notes and destroy them, until killed.
///
/// Calls the purge directly rather than waiting for a deadline. Whether a
/// redaction is *due* is a separate question with its own tests; what this
/// exists to interrupt is the purge itself, which writes across two databases
/// with no transaction spanning them.
///
/// Each note carries a distinctive word so the supervisor can ask the one
/// question that matters: is this content gone from every place it was, or
/// present in every place it was? A note that is gone from the log and still in
/// the search index is the failure the ordering is chosen to prevent.
fn redact_loop(root: &Path) -> Result<()> {
    use capture_core::body::{RedactBody, RedactScope};

    let mut log = Log::open(&root.join("events.db"))?;
    let ops = Ops::open(&root.join("ops.db"))?;
    let mut a = Author::new(&log)?;
    let mut n = highest_index(&log, &root.join("spool"))?;

    loop {
        // A note nobody else will produce, so the supervisor can look for it
        // by name and know what it should find.
        n += 1;
        let cap = a.author(payload(n), BASE_MS + n);
        let tgt = cap.header.id();
        log.append(&cap)?;

        let mut body = Vec::new();
        minicbor::encode(
            &RedactBody {
                nonce: [(n % 251) as u8; 16],
                scope: RedactScope::WholeNote,
                targets: alloc_one(tgt),
            },
            &mut body,
        )
        .expect("encoding into Vec is infallible");

        let mut red = a.author(body, BASE_MS + n);
        red.header.kind = Kind::REDACT;
        red.header.tgt = Some(tgt);
        // The header changed, so the signature has to cover the new id.
        red.sig = a.sign(&red.header);
        log.append(&red)?;

        // The projection has to know about the note before it can be purged
        // from it.
        let events = log.all()?;
        let generation = ops.generation()?;
        let g = projection::rebuild(
            &root.join("state.db"),
            &events,
            &log.aside_rows()?,
            generation,
            (BASE_MS + n) as i64,
        )?;
        ops.set_generation(g)?;
        let mut state = Store::open(&projection::generation_path(&root.join("state.db"), g))?;

        purge::execute(
            &mut log,
            &mut state,
            &ops,
            &root.join("blobs"),
            &red.header.id(),
            &tgt,
            BASE_MS + n,
        )?;
    }
}

fn alloc_one(id: capture_core::EventId) -> Vec<capture_core::EventId> {
    vec![id]
}

/// Append, then rebuild the projection into a fresh generation, until killed.
fn rebuild_loop(root: &Path) -> Result<()> {
    let mut log = Log::open(&root.join("events.db"))?;
    let ops = Ops::open(&root.join("ops.db"))?;
    let mut a = Author::new(&log)?;
    let mut n = a.seq;

    loop {
        for _ in 0..3 {
            n += 1;
            let e = a.author(payload(n), BASE_MS + n);
            log.append(&e)?;
        }
        let events = log.all()?;
        let current = ops.generation()?;
        let next = projection::rebuild(
            &root.join("state.db"),
            &events,
            &log.aside_rows()?,
            current,
            (BASE_MS + n) as i64,
        )?;
        ops.set_generation(next)?;

        // Prove the pointed-at generation is usable before moving on, so a
        // supervisor finding it broken knows the rebuild produced it that way
        // rather than the victim never having checked.
        let s = Store::open_read(&projection::generation_path(&root.join("state.db"), next))?;
        let _ = s.note_count()?;
        let _ = fold::fold(&events);
    }
}
