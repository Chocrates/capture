//! Getting events out of the store, and back in.
//!
//! # Export is incremental, and never has to stop anything
//!
//! The log is append-only and every event is immutable, so an export can be
//! taken while captures are happening and still be internally consistent — no
//! locking, no quiescing, no snapshot. Each run writes the events appended
//! since the last one into a dated file, and the whole directory is meant to be
//! copied offsite by ordinary tools with no coordination.
//!
//! How far each origin has been exported is tracked **per origin**, not as one
//! number. A single scalar is the same mistake as a single fold watermark:
//! events do not arrive in one global order, so one number either skips what
//! landed late or re-exports the archive every time something does.
//!
//! # Credential-flagged notes travel without their contents
//!
//! An export directory ends up somewhere redaction can never reach, so a note
//! the predicate flagged as credential-shaped must not carry its text there.
//!
//! It would be simpler to leave those events out of the file entirely, and that
//! is worse: an origin's sequence numbers would then have holes, and a hole is
//! the one thing this design treats as unrecoverable — a device summarises its
//! contents as a single number per origin and cannot say "everything up to 900
//! except 412".
//!
//! So the events are exported with their **bodies stripped**, exactly as a
//! redacted event is. The chain stays intact, each event still verifies (the
//! signature covers the header, and the header's body hash is checked only when
//! a body is present), and no credential leaves the machine. What is lost on
//! restore is the text of those specific notes, which is the intended trade.
//!
//! The predicate deliberately over-flags, so this is never silent: the count is
//! reported, the notes are listable, and `brain unflag` puts one back in the
//! next export.
//!
//! # Restore goes through the front door
//!
//! Restored events are offered to the same gate that any other incoming event
//! goes through — same verification, same roster check, same quarantine rules.
//! A special-purpose import would be a code path exercised only during a
//! disaster, which is to say a code path that has never been tested at the
//! moment it has to work.

use std::path::{Path, PathBuf};

use brain_core::{Dev, envelope::Envelope};
use brain_store::{
    Error, Result, StorePaths,
    gate::{Gate, Outcome},
    log::Log,
    ops::Ops,
    projection::Store,
    store,
};

use crate::read;
use brain_store::session::Session;

/// Magic at the head of every export file.
///
/// Cheap, and it means a truncated or unrelated file is refused with a sentence
/// rather than decoded into nonsense.
const MAGIC: &[u8; 8] = b"BRAINEX1";

// ---------------------------------------------------------------- export ----

/// Write everything appended since the last export.
pub fn export(paths: StorePaths, json: bool) -> Result<()> {
    // Read-only on the log; the writer is only needed for the watermark, which
    // lives in local state.
    let log = Log::open_read(&paths.events)?;
    let ops = Ops::open(&paths.ops)?;
    let state = read::open(&paths)?;

    let flagged: Vec<_> = state.credential_flagged()?;
    let strip: std::collections::BTreeSet<_> = flagged.iter().map(|(id, _)| *id).collect();

    let heads = ops.export_heads()?;
    let mut fresh: Vec<Envelope> = Vec::new();
    let mut stripped = 0usize;

    for e in log.all()? {
        let dev = dev_hex(&e.header.dev);
        if e.header.seq <= heads.get(&dev).copied().unwrap_or(0) {
            continue;
        }
        let touches = e.header.tgt.unwrap_or_else(|| e.header.id());
        let mut e = e;
        if strip.contains(&touches) && e.body.is_some() {
            // Kept in place, emptied. Dropping the event instead would leave a
            // hole in its origin's numbering that nothing can ever fill.
            e.body = None;
            stripped += 1;
        }
        fresh.push(e);
    }

    if fresh.is_empty() {
        if json {
            say!("{{\"events\":0,\"file\":null,\"stripped\":0}}");
        } else {
            say!("Nothing new to export.");
        }
        return Ok(());
    }

    std::fs::create_dir_all(&paths.export)?;
    let name = format!(
        "{}.brainlog",
        jiff::Timestamp::now().strftime("%Y%m%dT%H%M%SZ")
    );
    let file = paths.export.join(&name);
    write_file(&file, &fresh)?;

    // The watermark moves only after the bytes are on disk. The other order
    // would mean a crash between them silently skipping events forever.
    let mut moved: std::collections::BTreeMap<String, u64> = heads;
    for e in &fresh {
        let dev = dev_hex(&e.header.dev);
        let entry = moved.entry(dev).or_insert(0);
        *entry = (*entry).max(e.header.seq);
    }
    ops.set_export_heads(&moved)?;

    if json {
        say!(
            "{{\"events\":{},\"file\":\"{}\",\"stripped\":{}}}",
            fresh.len(),
            name,
            stripped
        );
        return Ok(());
    }

    say!("exported {} event(s) to {}", fresh.len(), file.display());
    if stripped > 0 {
        // Never silent. The predicate over-flags on purpose, so a note quietly
        // absent from every backup is the failure this reporting exists to
        // prevent.
        say!();
        say!(
            "{} event(s) were written without their text, from {} note(s) that look \
             like they hold credentials:",
            stripped,
            flagged.len()
        );
        for (_, slug) in &flagged {
            say!("  {slug}");
        }
        say!();
        say!("If one of those is an ordinary note:  brain unflag <name>");
    }
    Ok(())
}

fn write_file(path: &Path, events: &[Envelope]) -> Result<()> {
    use std::io::Write;
    let mut buf = Vec::new();
    buf.extend_from_slice(MAGIC);
    for e in events {
        let mut one = Vec::new();
        minicbor::encode(e, &mut one).expect("encoding into Vec is infallible");
        buf.extend_from_slice(&(one.len() as u32).to_le_bytes());
        buf.extend_from_slice(&one);
    }

    let mut f = std::fs::File::create(path)?;
    f.write_all(&buf)?;
    // An export that is not on disk is not a backup. This is the one place the
    // cost of an fsync is obviously worth it.
    f.sync_all()?;
    Ok(())
}

// --------------------------------------------------------------- restore ----

/// Read every export file in a directory, oldest first.
fn read_dir(dir: &Path) -> Result<Vec<Envelope>> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(|_| Error::NotFound(format!("no export directory at {}", dir.display())))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "brainlog"))
        .collect();
    files.sort();

    if files.is_empty() {
        return Err(Error::NotFound(format!(
            "no export files in {}",
            dir.display()
        )));
    }

    let mut out = Vec::new();
    for f in &files {
        out.extend(read_file(f)?);
    }
    Ok(out)
}

fn read_file(path: &Path) -> Result<Vec<Envelope>> {
    let bytes = std::fs::read(path)?;
    if bytes.len() < MAGIC.len() || &bytes[..MAGIC.len()] != MAGIC {
        return Err(Error::Integrity(format!(
            "{} is not an export file",
            path.display()
        )));
    }

    let mut out = Vec::new();
    let mut i = MAGIC.len();
    while i + 4 <= bytes.len() {
        let n = u32::from_le_bytes(bytes[i..i + 4].try_into().unwrap()) as usize;
        i += 4;
        if i + n > bytes.len() {
            // A torn tail. The file was being written when something stopped,
            // so the records before it are still good and the partial one was
            // never complete enough to mean anything.
            break;
        }
        match minicbor::decode::<Envelope>(&bytes[i..i + n]) {
            Ok(e) => out.push(e),
            Err(_) => {
                return Err(Error::Integrity(format!(
                    "{} holds a record this build cannot decode",
                    path.display()
                )));
            }
        }
        i += n;
    }
    Ok(out)
}

/// Rebuild a store from exported events.
pub fn restore(paths: StorePaths, from: &Path, ignore_after: Option<u64>) -> Result<()> {
    let events = read_dir(from)?;

    if !store::exists(&paths) {
        return Err(Error::StoreMissing(paths.root.clone()));
    }

    let mut s = Session::open(paths)?;
    let ops = Ops::open(&s.paths.ops)?;

    let events = match ignore_after {
        None => events,
        Some(cutoff) => {
            // Deliberately not the default. Ordinary restore must apply every
            // redaction, or restoring an old backup would resurrect content
            // that was destroyed on purpose. This mode exists for the opposite
            // mistake — a redaction fired at the wrong note — and the two
            // intents are kept apart rather than guessed between.
            let before = events.len();
            let kept: Vec<Envelope> = events
                .into_iter()
                .filter(|e| {
                    !(e.header.kind == brain_core::event::Kind::REDACT
                        && e.header.hlc.wall > cutoff)
                })
                .collect();
            say!(
                "ignoring {} redaction(s) issued after the cutoff",
                before - kept.len()
            );
            kept
        }
    };

    let mut took = 0usize;
    let mut already = 0usize;
    let mut refused = 0usize;
    let mut aside = 0usize;
    let now = jiff::Timestamp::now().as_millisecond().max(0) as u64;

    {
        // The ordinary ingest path, not an importer. A restore route exercised
        // only during a disaster is one that has never run when it mattered.
        let own = s.identity.public().to_bytes();
        let mut gate = Gate::open(&mut s.log, &mut s.state, &ops, own)?;
        for e in &events {
            match gate.offer(e, now)? {
                Outcome::Took(brain_store::log::Ingested::Duplicate) => already += 1,
                Outcome::Took(_) => took += 1,
                Outcome::SetAside(_) => aside += 1,
                Outcome::Refused(_) => refused += 1,
                Outcome::Paused => {
                    return Err(Error::Integrity(
                        "the export does not begin with a roster root, so nothing in it can be \
                         verified"
                            .into(),
                    ));
                }
            }
        }
    }

    // A fresh origin, before anything local can be written.
    //
    // The sequence counter has just moved backwards relative to what peers may
    // already hold. Authoring under the old origin would reuse positions they
    // have, which loses the original event and the new one, in both directions,
    // with nothing to notice it. A new incarnation gives a new origin whose
    // numbering starts clean.
    //
    // The claim is the first thing written under it. Membership is keyed by
    // public key, so this device is still a member — but nothing else in the
    // log says which key the new origin belongs to, and without that every
    // replica that later sees its events refuses them permanently. The failure
    // is invisible locally, because local verification uses the local key.
    let claim = s.reincarnate()?;
    say!("new origin {} claimed", short(&claim));

    // Everything is in the log; recompute the projection from scratch rather
    // than patching it, because a restore touches every note at once and a
    // rebuild is what the projection is for.
    let all = s.log.all()?;
    let generation = ops.generation()?;
    let next = brain_store::projection::rebuild(
        &s.paths.state,
        &all,
        generation,
        jiff::Timestamp::now().as_millisecond().max(0),
    )?;
    ops.set_generation(next)?;

    let store = Store::open_read(&brain_store::projection::generation_path(
        &s.paths.state,
        next,
    ))?;

    say!("restored {} event(s) from {}", events.len(), from.display());
    say!("  {took} new, {already} already held");
    if aside > 0 {
        say!("  {aside} set aside — authentic, but not usable here");
    }
    if refused > 0 {
        say!("  {refused} refused — could not be authenticated");
    }
    say!("  {} note(s)", store.note_count()?);
    Ok(())
}

fn short(d: &Dev) -> String {
    d.short().iter().map(|b| format!("{b:02x}")).collect()
}

fn dev_hex(d: &Dev) -> String {
    d.0.iter().map(|b| format!("{b:02x}")).collect()
}
