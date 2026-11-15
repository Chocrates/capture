//! Operating the store: log, status, quarantine, rejected, rebuild, set.
//!
//! # Health reporting is tiered, and says so when nothing is wrong
//!
//! Three tiers: conditions needing action, conditions worth knowing, and
//! diagnostics. An undifferentiated list of warnings trains the reader to
//! ignore all of them, and most of what this reports is the system working
//! correctly rather than failing.
//!
//! Two rules the wording has to keep:
//!
//! - **State the healthy case explicitly.** An empty surface is ambiguous
//!   between "nothing is wrong" and "nothing was checked", and manufacturing
//!   that ambiguity is a cost this system would be generating for free.
//! - **Never report as verified what has not been checked.** `status` reads
//!   counters; it does not open a single event. Saying the archive is intact on
//!   that basis would be an unqualified positive about something unexamined,
//!   which in a system built to compensate for unreliable memory is worse than
//!   admitting the gap. So it names `capture verify` rather than implying it.
//!
//! # Nothing here scores the user
//!
//! Counts describe the data — how many notes, how many quarantined events.
//! Nothing describes the person: no streak, no capture target, no completion
//! figure. Repeated performance failure is a documented mechanism behind the
//! anxiety this design exists for, so a status page that manufactures a running
//! failure signal works directly against its own purpose.

use capture_core::event::Kind;
use capture_store::{
    Error, Result, StorePaths,
    lock::{self, Writer},
    log::Log,
    ops::Ops,
    projection::{self, Store},
    store,
};

use crate::json::{Json, View, answer, arr, obj};
use crate::read;

// The table, its lookup and the value in force all live in `capture-store`,
// beside the function that validates against them. Every surface writes a
// setting through that, and a copy here would be a second answer to "is this a
// real key" — which is what it was.
use capture_store::settings;

/// The value in force for a setting, chosen or compiled in.
pub fn effective(state: &Store, key: &str) -> Result<Option<String>> {
    settings::effective(state, key)
}

// ------------------------------------------------------------------ log ----

/// Every event that touched a note, oldest first.
///
/// The audit trail for one note. `show` renders the current state; this renders
/// how it got there, which is the only way to answer "when did this change and
/// which device did it".
pub fn log(paths: &StorePaths, name: &str, json: bool) -> Result<()> {
    let state = read::open(paths)?;
    let Some(tgt) = state.resolve(name)? else {
        return Err(Error::NotFound(name.to_string()));
    };

    // Read from the log, not the projection: the projection holds the versions
    // it folded, and the question here is what was actually written.
    let log = Log::open_read(&paths.events)?;
    let mut events: Vec<_> = log
        .all()?
        .into_iter()
        .filter(|e| e.header.tgt == Some(tgt) || e.header.id() == tgt)
        .collect();
    events.sort_by_key(|e| e.header.hlc);

    answer(
        &History {
            name: name.to_string(),
            events: events
                .iter()
                .map(|e| Written {
                    id: e.header.id().0,
                    kind: e.header.kind,
                    seq: e.header.seq,
                    origin: hex(&e.header.dev.short()),
                    wall: e.header.hlc.wall,
                })
                .collect(),
        },
        json,
    );
    Ok(())
}

/// One note's events, oldest first.
struct History {
    name: String,
    events: Vec<Written>,
}

/// One event, as the audit trail names it.
struct Written {
    id: [u8; 32],
    kind: Kind,
    seq: u64,
    origin: String,
    wall: u64,
}

impl View for History {
    fn json(&self) -> Json {
        // The name is the question.
        let History { name: _, events } = self;
        arr(events.iter().map(
            |Written {
                 id,
                 kind,
                 seq,
                 origin,
                 wall,
             }| {
                obj([
                    // Eight bytes here and four in prose: a script may need
                    // to tell apart what a person never will.
                    ("id", hex(&id[..8]).into()),
                    ("kind", kind_name(*kind).into()),
                    ("seq", seq.into()),
                    ("origin", origin.into()),
                    ("wall_ms", wall.into()),
                ])
            },
        ))
    }

    fn prose(&self) {
        let History { name, events } = self;
        say!("{name}");
        for e in events {
            say!(
                "  {}  {:<14} {} seq {}  {}",
                hex(&e.id[..4]),
                kind_name(e.kind),
                e.origin,
                e.seq,
                when(e.wall)
            );
        }
    }
}

fn kind_name(k: Kind) -> &'static str {
    match k {
        Kind::CAPTURE => "capture",
        Kind::EDIT => "edit",
        Kind::TAG => "tag",
        Kind::UNTAG => "untag",
        Kind::ALIAS => "alias",
        Kind::PIN => "pin",
        Kind::MUTE => "mute",
        Kind::RESOLVE => "resolve",
        Kind::REDACT => "redact",
        Kind::REDACT_CANCEL => "redact-cancel",
        Kind::TOMBSTONE => "tombstone",
        Kind::DEVICE_ADD => "device-add",
        Kind::ORIGIN_CLAIM => "origin-claim",
        Kind::SETTING => "setting",
        Kind::UNFLAG => "unflag",
        Kind::PROMOTE => "promote",
        Kind::INTENT => "intent",
        Kind::REVIEW => "review",
        Kind::OCC_MOVE => "occ-move",
        Kind::OCC_EDIT => "occ-edit",
        Kind::OCC_CANCEL => "occ-cancel",
        Kind::OCC_DONE => "occ-done",
        Kind::OCC_UNDONE => "occ-undone",
        Kind::ITEM_DONE => "item-done",
        Kind::ITEM_UNDONE => "item-undone",
        Kind::PARENT => "parent",
        Kind::ROSTER_KEY => "roster-key",
        Kind::DEVICE_RETIRE => "device-retire",
        Kind::DEVICE_READMIT => "device-readmit",
        Kind::ENTITY_RENAME => "entity-rename",
        Kind::ENTITY_MERGE => "entity-merge",
        Kind::ENTITY_BIND => "entity-bind",
        // A kind this build has never heard of. Named as such rather than
        // rendered blank: it is almost certainly a newer client, which is a
        // different fact from a corrupt event.
        //
        // Every kind this build *does* know is named above. Fifteen of them
        // were missing and printed "unknown" here — `promote`, `intent`,
        // `review`, every `occ_*`, `parent`, both `item_*` and the roster
        // kinds — so `capture log` reported a newer client for events this
        // build had written itself.
        _ => "unknown",
    }
}

// --------------------------------------------------------------- status ----

/// What the store holds, and anything that needs attention.
pub fn status(paths: &StorePaths) -> Result<()> {
    if !store::exists(paths) {
        return Err(Error::StoreMissing(paths.root.clone()));
    }

    let log = Log::open_read(&paths.events)?;
    let ops = Ops::open_read(&paths.ops)?;
    let generation = ops.generation()?;
    let state = read::open(paths)?;

    say!("store       {}", paths.root.display());
    say!("notes       {}", state.note_count()?);
    say!("events      {}", log.count()?);
    say!("generation  {generation}");

    // The kernel decides whether the store is held, not the lock file — its
    // contents outlive the process that wrote them, and reporting a dead
    // process as the writer is the pid-file failure the advisory lock avoids.
    let lock_path = paths.root.join("writer.lock");
    match Writer::acquire(&lock_path, "status")? {
        Some(_) => say!("writer      none"),
        None => say!(
            "writer      {}",
            lock::holder(&lock_path).unwrap_or_else(|| "another process".into())
        ),
    }

    // The capture gesture, and whether it is still reaching anything.
    //
    // Reported here because a chord that has quietly stopped working is
    // invisible everywhere else: the desktop still lists the binding, nothing
    // logs an error, and the person who set it up goes on believing they are
    // capturing things.
    let mechanism = capture_store::hotkey::select(&capture_store::hotkey::Host);
    let chord = capture_store::hotkey::DEFAULT_CHORD.human();
    say!("chord       {chord} via {}", mechanism.name());

    // A default is a rule; a setting is a decision. §8.9 keeps them apart, and
    // this is the line where the difference becomes visible: a machine-chosen
    // notifier reported as "set by you" is a preference nobody made that can
    // never be centrally improved.
    match capture_store::notifier::chosen(&state)? {
        Some(who) => say!("notifier    {who} — your choice"),
        // Which device, and by what rule. "The only one paired" was printed
        // here with three devices in the roster.
        None => {
            let me = capture_store::identity::Identity::load(paths)?
                .public()
                .to_bytes();
            let who =
                capture_store::notifier::default_here(paths, &me, crate::read::now_ms() as u64)?;
            say!(
                "notifier    {}{} — {}, a default, not a setting",
                short_key(&who),
                if who == me { " (this device)" } else { "" },
                capture_store::notifier::default_rule()
            );
        }
    }

    // ---- tier one: things that want a decision -----------------------------
    let mut attention: Vec<String> = Vec::new();
    // Declared with the other, rather than beside the first thing that fills
    // it, because the sync condition below needs both: a round that stopped a
    // day ago is worth knowing and one that stopped a week ago wants a
    // decision, and it has to be said *before* any peer is described.
    let mut worth_knowing: Vec<String> = Vec::new();

    let health = capture_store::gesture::health(
        mechanism != capture_store::hotkey::Mechanism::None,
        crate::read::now_ms() as u64,
        ops.gesture_last_fired()?,
        // Derived from the log rather than read from a counter kept beside it:
        // every event names the device that wrote it and when.
        log.last_wall(&capture_store::identity::Identity::load(paths)?.dev())?,
    );
    if let Some(m) = health.message(&chord) {
        attention.push(m);
    }

    // What the overlay measured about itself. A diagnostic, not a score: it
    // describes the software's behaviour, and the moment it starts describing
    // the person it has no business on this page.
    if let Some(l) = ops.latency()?.summary() {
        say!("overlay     {l}");
        say!("            (request to first frame — the compositor's share is not visible here)");
    }

    // Redactions waiting, and whether anything is looking at them.
    //
    // The pair matters more than either alone. A queue with items in it is
    // normal; a queue with overdue items and a last-processed time from three
    // weeks ago means destruction is not happening while every other surface
    // reports it complete — the failure that put this queue in the projection
    // rather than in local state.
    let now = crate::read::now_ms() as u64;
    let queue = state.redact_queue()?;
    let pending: Vec<_> = queue.iter().filter(|(_, _, _, c)| !c).collect();
    if !pending.is_empty() {
        let overdue = pending.iter().filter(|(_, _, at, _)| *at <= now).count();
        say!("redactions  {} queued", pending.len());
        if overdue > 0 {
            attention.push(format!(
                "{overdue} redaction(s) are past their window and have not been carried out.\n\
                 Run any command that writes, or start the overlay, to complete them."
            ));
        }
    }

    match ops.due_last_processed()? {
        Some(at) => {
            let ago = now.saturating_sub(at);
            say!("due work    last processed {} ago", ms(ago));
        }
        // Never having run is only worth saying when there is something waiting
        // on it.
        None if !pending.is_empty() => {
            attention.push("scheduled work has never been processed on this device".into())
        }
        None => {}
    }

    let stale = ops.stale_export_count()?;
    if stale > 0 {
        // Tier one, and acknowledgeable rather than self-clearing: the files
        // are somewhere this program cannot look, so it can never verify they
        // were dealt with. Clearing it automatically would be claiming to know
        // something it does not.
        attention.push(format!(
            "{stale} redaction(s) destroyed content that older export files still contain.\n\
             Redaction cannot reach a backup. Delete or re-create the exports in\n\
             {}, then: capture exports-pruned",
            paths.export.display()
        ));
    }

    let unfinished = ops.unfinished_purges()?;
    if !unfinished.is_empty() {
        attention.push(format!(
            "{} redaction(s) were interrupted partway through destroying content.\n\
             They will be finished by the next command that writes.",
            unfinished.len()
        ));
    }

    let rejected = ops.rejected_total()?;
    if rejected > 0 {
        // Never folded into a general total. An event that failed
        // authentication is a security signal, and burying it alongside version
        // skew is exactly how it goes unnoticed.
        for (reason, n) in ops.rejected_counts()? {
            attention.push(format!("{n} event(s) failed authentication ({reason})"));
        }
    }
    if capture_store::spool::in_use(&paths.spool)? {
        attention.push("captures are being spooled — the store could not be written".into());
    }
    // Redactions that have not reached every device they were issued against.
    //
    // The pending set was fixed when the redaction was issued and is not read
    // from the roster, so a device retired since then is still listed as never
    // acknowledged. That is deliberate: the realistic order is that the phone
    // is stolen, you retire it, and *then* you redact — and reporting
    // completion because the phone is no longer a member would be reporting
    // success for the one device that never received anything.
    let outstanding = ops.redactions_outstanding()?;
    if !outstanding.is_empty() {
        let peers: std::collections::BTreeSet<_> =
            outstanding.iter().flat_map(|(_, p)| p.clone()).collect();
        attention.push(format!(
            "{} redaction(s) have not been acknowledged by {} device(s): {}.\n\
             Until they are, the content may still exist on those devices.",
            outstanding.len(),
            peers.len(),
            peers.iter().map(short_key).collect::<Vec<_>>().join(", ")
        ));
    }

    // Whether sync is running at all, before anything about a peer.
    //
    // This was missing and the omission had a cost. A round that never ran and
    // a round that reached nobody need different remedies — a daemon to start,
    // or a network to fix — and the peer rows below describe them identically.
    // So somebody whose `capture-syncd` had simply never been started read
    // "device d4389d6f has never been reached", went to look at their Wi-Fi,
    // and wrote "sync not running?" in a notebook with a question mark.
    //
    // The phone had been asking both questions and saying which; this one and
    // the window had not. The decision now lives in `capture-store` so all
    // three give one answer.
    // How many *other* devices are paired, from the roster rather than from
    // `ops`. A device paired and never yet dialled has a roster entry and no
    // `peer_state` row, and that is exactly the case worth reporting — asking
    // `ops` would answer it with silence.
    let others = paired_others(paths)?;

    // Whether sync is running at all, before any peer is described.
    //
    // The order is the point. "device X has never been reached" is a fact about
    // a peer and reads as a fact about the network, so somebody whose
    // `capture-syncd` was never started goes to look at their Wi-Fi. The round
    // question has a different remedy and has to come first, which is what
    // `ops::round_ran` was recorded separately *for* — its own comment says a
    // surface should be able to tell "nobody was home" from "nobody knocked",
    // and this surface could not.
    if let Some((tier, text)) = capture_store::syncstate::sync_state(&ops, others, now)?.message() {
        if tier == "attention" {
            attention.push(text);
        } else {
            worth_knowing.push(text);
        }
    }

    // A device that has not been reached in over a week.
    //
    // Promoted deliberately (§4.5): a phone that has silently stopped syncing
    // holds captures that exist on exactly one device, and the longer that
    // goes unnoticed the more of them there are. It says *when* rather than
    // only that it is stale, because "three weeks" and "eight days" call for
    // different reactions.
    for p in ops.peer_addresses()? {
        let stale = capture_store::syncstate::peer_is_stale(p.last_seen, now);
        if stale {
            match p.last_seen {
                Some(t) => attention.push(format!(
                    "device {} has not been reached in {}.\n\
                     Anything captured on it since then exists only there.",
                    short_key(&p.key),
                    ms(now.saturating_sub(t))
                )),
                None => attention.push(format!(
                    "device {} has never been reached.",
                    short_key(&p.key)
                )),
            }
        }
    }

    let identity = capture_store::identity::Identity::load(paths)?;

    // A device in the roster that this one neither paired nor admitted itself.
    //
    // Usually ordinary — a pairing performed on another device, which is the
    // whole point of the roster replicating. But it is also what a compromised
    // device admitting an attacker looks like, and §5.3 is explicit that
    // nothing prevents that: the remedy is that it is *seen*. Nothing here can
    // tell the two apart, so it asks rather than deciding, and stays until
    // answered.
    let own = std::collections::BTreeSet::from([identity.public().to_bytes()]);
    let unexpected = ops.unexpected_admissions(&roster_now(paths)?.keys, &own)?;
    for key in &unexpected {
        attention.push(format!(
            "a device was admitted that this one did not pair or admit: {}\n\
             If you paired it on another device, this is expected. If not, it can\n\
             read everything in the archive. Then: capture admission-seen {}",
            short_key(key),
            short_key(key)
        ));
    }

    let w = identity.watermark();
    let lead = capture_core::hlc::HlcState::restore(w.hlc_wall, w.hlc_counter)
        .lead_ms(crate::read::now_ms() as u64);
    if lead > capture_core::hlc::LEAD_REPORT_MS {
        attention.push(format!(
            "this device's clock reads {} behind the events it is writing — \
             ordering is distorted until it is corrected (captures still work)",
            ms(lead)
        ));
    }

    // ---- tier two: worth knowing, no action ---------------------------------

    let quarantined = state.quarantined()?;
    if !quarantined.is_empty() {
        // Described by cause. Most of these are a newer client writing kinds
        // this build has no rule for, which is the system working.
        let mut by_reason: std::collections::BTreeMap<String, usize> = Default::default();
        for (_, reason) in &quarantined {
            *by_reason.entry(reason.clone()).or_default() += 1;
        }
        for (reason, n) in by_reason {
            worth_knowing.push(format!("{n} event(s) set aside: {reason}"));
        }
    }
    // Older notes that now read as sun alarms. A grammar bump turned "at
    // sunset" in a backlog note into something that rings, and the person
    // should hear that once rather than discover it at dusk (D8). Shown for a
    // week after the last one, from local state.
    if let Some((at, slugs)) = capture_store::promote::sun_reread(&ops)?
        && !slugs.is_empty()
        && (now as i64) - at <= capture_store::promote::SUN_REREAD_SHOWN_MS
    {
        let shown: Vec<_> = slugs.iter().take(5).map(String::as_str).collect();
        worth_knowing.push(format!(
            "{} older note(s) now read as sun times and will ring: {}{}",
            slugs.len(),
            shown.join(", "),
            if slugs.len() > shown.len() {
                ", …"
            } else {
                ""
            }
        ));
    }
    // A cancellation that came too late.
    //
    // §1.1 asks for a cancel arriving after its target has been destroyed to be
    // reported rather than silently discarded. Somebody who cancels believes
    // the content survived — on the device they cancelled from it may have, and
    // on this one it did not. Both devices otherwise report success and neither
    // mentions the other.
    //
    // No remedy offered, because there is none: the content is gone. A snapshot
    // may exist, and that is a separate mechanism with its own window and its
    // own command; pointing at it here would imply the two are connected.
    let late = state.late_cancels()?;
    if !late.is_empty() {
        let mut names: Vec<String> = Vec::new();
        for tgt in &late {
            names.push(state.slug_of(tgt)?.unwrap_or_else(|| "a note".into()));
        }
        attention.push(format!(
            "{} cancellation(s) arrived after the content had already been destroyed \
             here: {}.\nNothing was recovered. Another device may have cancelled in \
             time; this one did not.",
            late.len(),
            names.join(", ")
        ));
    }

    // Destruction the ceiling stopped. Tier one because it is waiting on a
    // person: §1.1 holds what is over the ceiling "pending explicit
    // confirmation", and a hold nobody is told about is an archive stuck in a
    // state no surface explains.
    let held_redactions = ops.held_redactions()?;
    if !held_redactions.is_empty() {
        let mut which: Vec<String> = held_redactions
            .iter()
            .map(|(r, _)| capture_store::mutate::short(r))
            .collect();
        which.sort();
        which.dedup();
        attention.push(format!(
            "{} destruction(s) held: one origin named a great deal within an hour.\n\
             Nothing was destroyed. If you meant it:  capture redact --release <id>\n\
             Waiting: {}",
            held_redactions.len(),
            which.join(", ")
        ));
    }

    let snapshots = ops.snapshot_count(now)?;
    if snapshots > 0 {
        // Worth knowing rather than worth acting on. These are plaintext copies
        // of destroyed content, held deliberately and briefly, and somebody
        // should be able to find out they exist without going looking.
        worth_knowing.push(format!(
            "{snapshots} copy(ies) of destroyed content are kept locally, and expire on their own"
        ));
    }

    let held = state.held_count()?;
    if held > 0 {
        // Names where to look. This said only the count, and `held_count` was
        // the only accessor in the workspace — so somebody reading it could
        // not find out which event, of what kind, or why. A condition with
        // nowhere to go gets read once and skipped afterwards.
        worth_knowing.push(format!(
            "{held} event(s) stored but not folded — this build has no rule for them.\n\
             Which ones:  capture held"
        ));
    }
    let pending = log.pending()?;
    if !pending.is_empty() {
        worth_knowing.push(format!(
            "{} event(s) waiting on earlier ones to arrive",
            pending.len()
        ));
    }

    say!();
    if attention.is_empty() {
        // Said out loud. An empty section reads as "nothing was checked".
        say!("Nothing needs your attention.");
    } else {
        say!("Needs attention");
        for a in &attention {
            // Indented as a block. A condition worth reporting is often worth
            // two sentences, and a continuation line flush against the margin
            // reads as a separate item.
            for (i, line) in a.lines().enumerate() {
                say!("  {}{line}", if i == 0 { "" } else { "  " });
            }
        }
    }

    if !worth_knowing.is_empty() {
        say!();
        say!("Worth knowing");
        for w in &worth_knowing {
            for (i, line) in w.lines().enumerate() {
                say!("  {}{line}", if i == 0 { "" } else { "  " });
            }
        }
    }

    // ---- settings, with provenance ------------------------------------------
    say!();
    say!("Settings");
    let chosen = state.settings()?;
    for s in settings::KNOWN {
        let (key, what) = (s.key, s.what);
        // A setting with no compiled-in value is decided by a rule, and the
        // rule is what to print in the column where a value would go.
        let default = s.default.unwrap_or("by rule");
        let picked = chosen.iter().find(|(k, _, _)| k == key);
        match picked {
            Some((_, Some(v), _)) => say!("  {key:<18} {v:<10} set by you — {what}"),
            // Reverting is a decision too, and a different one from never
            // having chosen: a future change to the default should follow the
            // second and not the first.
            Some((_, None, _)) => {
                say!("  {key:<18} {default:<10} default, restored by you — {what}")
            }
            None => say!("  {key:<18} {default:<10} default — {what}"),
        }
    }

    // ---- what has not been checked ------------------------------------------
    say!();
    say!("Not checked here: signatures, body hashes, and origin chains.");
    say!("  capture verify");
    Ok(())
}

// ----------------------------------------------------- quarantine, rejected ----

/// Membership as the log says it is.
///
/// Built from the log rather than read from the projection, for the same
/// reason `Gate` does it: the projection may be absent, stale, or mid-rebuild,
/// and it is only written when a gate opens — which a local capture never
/// does. A surface reading it would have reported an empty roster on a device
/// that had never ingested from a peer, which is every device until the first
/// sync.
/// How many other devices are paired, from the roster.
pub fn paired_others(paths: &StorePaths) -> Result<usize> {
    let me = capture_store::identity::Identity::load(paths)?
        .public()
        .to_bytes();
    Ok(roster_now(paths)?.keys.iter().filter(|k| **k != me).count())
}

fn roster_now(paths: &StorePaths) -> Result<capture_core::roster::Roster> {
    let log = capture_store::log::Log::open_read(&paths.events)?;
    Ok(capture_core::roster::build(&log.all()?))
}

/// Every device in the roster, and when this one last reached it.
/// Remember an address for sync to try. Device-local: it never enters the log,
/// so it is not sent to any other device.
pub fn peers_add(paths: &StorePaths, addr: &str) -> Result<()> {
    Ops::open(&paths.ops)?.give_address(addr)?;
    say!("Sync will try {addr} each round.");
    say!("Whose it is is learned when it first answers.");
    Ok(())
}

pub fn peers(paths: &StorePaths, json: bool) -> Result<()> {
    // Read-only. This command reads and the §8.8 rule is that reads go direct;
    // holding a writable handle to answer a question about peers is that rule
    // broken in miniature.
    let ops = Ops::open_read(&paths.ops)?;
    let identity = capture_store::identity::Identity::load(paths)?;
    let me = identity.public().to_bytes();

    // As this device holds it, which is not quite as the log folds it: a
    // device refuses to apply a retirement of itself that it did not issue
    // (§5.3), and the gate already honours that. Without the same step here a
    // device another had retired listed every peer but itself — no "this
    // device" row at all — while still writing and syncing as a member.
    let mut roster = roster_now(paths)?;
    roster.honour_self(&me);
    // Devices only. An agent is listed by `capture agent list`; here it would
    // be a peer that is never reached, and a reason to worry that is not one.
    let members: std::collections::BTreeSet<[u8; 32]> = roster.devices().copied().collect();
    let addresses = ops.peer_addresses()?;
    let now = crate::read::now_ms() as u64;

    // What the rows cannot say.
    //
    // "never reached" is a fact about a peer and reads as a fact about the
    // network. Whether a round has ever run is a different question with a
    // different remedy, and without it somebody whose daemon was never started
    // goes looking at their Wi-Fi. Beside the rows rather than instead of them:
    // the two together are a complete sentence.
    let others = members.iter().filter(|k| **k != me).count();
    let sync = if others == 0 {
        None
    } else {
        capture_store::syncstate::sync_state(&ops, others, now)?
            .message()
            .map(|(_, text)| text)
    };

    answer(
        &Peers {
            peers: members
                .iter()
                .map(|k| {
                    let p = addresses.iter().find(|p| &p.key == k);
                    Peer {
                        key: short_key(k),
                        me: k == &me,
                        addr: p.and_then(|p| p.addr.clone()),
                        last_seen: p.and_then(|p| p.last_seen),
                        seen: p.is_some(),
                    }
                })
                .collect(),
            now,
            sync,
        },
        json,
    );
    Ok(())
}

/// Every device in the roster, and whether sync has ever reached any of them.
struct Peers {
    peers: Vec<Peer>,
    now: u64,
    /// The sentence about sync as a whole, where there is one to say.
    sync: Option<String>,
}

struct Peer {
    key: String,
    me: bool,
    addr: Option<String>,
    last_seen: Option<u64>,
    /// Whether it has ever been seen on any network, address or not.
    seen: bool,
}

impl View for Peers {
    fn json(&self) -> Json {
        let Peers {
            peers,
            now: _, // `last_seen` is an instant; "ago" is for people
            sync,
        } = self;
        // An object rather than the bare array it was, because the sync state
        // is about the roster rather than any one row of it.
        obj([
            (
                "peers",
                arr(peers.iter().map(
                    |Peer {
                         key,
                         me,
                         addr,
                         last_seen,
                         seen,
                     }| {
                        obj([
                            ("key", key.into()),
                            ("self", (*me).into()),
                            ("addr", addr.into()),
                            ("last_seen", (*last_seen).into()),
                            ("seen", (*seen).into()),
                        ])
                    },
                )),
            ),
            ("sync", sync.into()),
        ])
    }

    fn prose(&self) {
        let Peers { peers, now, sync } = self;
        for p in peers {
            if p.me {
                say!("  {}  this device", p.key);
                continue;
            }
            // The address and the last contact are separate facts, and a peer
            // can have either without the other: one that only ever dials in
            // is demonstrably alive at no address this device can call.
            if !p.seen {
                say!("  {}  never seen on any network", p.key);
                continue;
            }
            let at = p.addr.clone().unwrap_or_else(|| "no address".into());
            match p.last_seen {
                Some(t) => say!(
                    "  {}  {at} — last reached {} ago",
                    p.key,
                    ms(now.saturating_sub(t))
                ),
                None => say!("  {}  {at} — never reached", p.key),
            }
        }
        if peers.len() == 1 {
            say!();
            say!("No other devices. To add one:  capture pair");
            return;
        }
        if let Some(text) = sync {
            say!();
            say!("{text}");
        }
    }
}

/// Events that are authentic but this device cannot use.
pub fn quarantine(paths: &StorePaths, json: bool) -> Result<()> {
    let state = read::open(paths)?;
    answer(
        &SetAside {
            rows: state.quarantined()?,
        },
        json,
    );
    Ok(())
}

struct SetAside {
    rows: Vec<(capture_core::id::EventId, String)>,
}

impl View for SetAside {
    fn json(&self) -> Json {
        let SetAside { rows } = self;
        arr(rows
            .iter()
            .map(|(id, reason)| obj([("id", hex(&id.0[..8]).into()), ("reason", reason.into())])))
    }

    fn prose(&self) {
        let SetAside { rows } = self;
        if rows.is_empty() {
            say!("Nothing is quarantined.");
            return;
        }
        say!("Set aside — authentic, and holding their positions, but unusable here.");
        say!();
        for (id, reason) in rows {
            say!("  {}  {reason}", hex(&id.0[..8]));
        }
    }
}

/// Events stored but not folded, and why.
///
/// `capture status` could report a count and nothing could report anything
/// else: `held_count` was the only accessor in the workspace. So a line saying
/// "this build has no rule for them" had no next step, and nobody could tell
/// whether it meant a newer peer had written something this build predates — or
/// whether the count was simply wrong, which for every archive ever rebuilt it
/// was, because the fold filed its own roster key as an unknown kind.
///
/// The kind comes from the log rather than the projection, because what the
/// event *is* has always lived there. Naming it is the whole point: "unknown
/// kind 33" is a newer build on another device and will resolve on upgrade,
/// where "unknown kind 25" was this one failing to recognise a roster key.
pub fn held(paths: &StorePaths, json: bool) -> Result<()> {
    let state = read::open(paths)?;
    let rows = state.held()?;
    let log = Log::open_read(&paths.events)?;

    // Every event once, so the kind of a held id can be looked up. A held set
    // is a handful at most — it is bounded by what this build cannot read —
    // and anything cleverer would be optimising a diagnostic.
    let kinds: std::collections::BTreeMap<_, _> = log
        .all()?
        .iter()
        .map(|e| (e.header.id(), e.header.kind))
        .collect();

    answer(
        &Held {
            rows: rows
                .into_iter()
                .map(|(id, reason)| (hex(&id.0[..8]), kinds.get(&id).map(|k| k.0), reason))
                .collect(),
        },
        json,
    );
    Ok(())
}

/// Held events: id, kind where the log knows it, and why.
struct Held {
    rows: Vec<(String, Option<u16>, String)>,
}

impl View for Held {
    fn json(&self) -> Json {
        let Held { rows } = self;
        arr(rows.iter().map(|(id, kind, reason)| {
            obj([
                ("id", id.into()),
                ("reason", reason.into()),
                // `0` for one the log has no record of, as before — no kind
                // is numbered zero.
                ("kind", kind.unwrap_or(0).into()),
            ])
        }))
    }

    fn prose(&self) {
        let Held { rows } = self;
        if rows.is_empty() {
            say!("Nothing is held.");
            say!();
            say!("Every event in the log has a rule this build understands.");
            return;
        }

        say!("Stored, authentic, and not folded — this build has no rule for them.");
        say!();
        for (id, kind, reason) in rows {
            match kind {
                Some(k) => say!("  {id}  kind {k}  {reason}"),
                None => say!("  {id}  kind unknown  {reason}"),
            }
        }
        say!();
        say!("A kind this build does not have is usually a newer one on another");
        say!("device. These are re-examined on every rebuild, so an upgrade picks");
        say!("them up without anything being lost.");
    }
}

/// Events that failed authentication.
pub fn rejected(paths: &StorePaths, json: bool) -> Result<()> {
    let ops = Ops::open_read(&paths.ops)?;
    answer(
        &Refused {
            counts: ops.rejected_counts()?,
        },
        json,
    );
    Ok(())
}

struct Refused {
    counts: std::collections::BTreeMap<String, u64>,
}

impl View for Refused {
    fn json(&self) -> Json {
        let Refused { counts } = self;
        arr(counts
            .iter()
            .map(|(reason, n)| obj([("reason", reason.into()), ("count", n.into())])))
    }

    fn prose(&self) {
        let Refused { counts } = self;
        if counts.is_empty() {
            say!("No event has failed authentication.");
            return;
        }
        say!("Refused — not authenticated, holding no position in any sequence.");
        say!();
        for (reason, n) in counts {
            say!("  {n:<6} {reason}");
        }
    }
}

// --------------------------------------------------------------- rebuild ----

/// Throw the projection away and compute it again from the log.
///
/// This is the entire migration strategy. Nothing in the projection is
/// authoritative, so when its shape needs to change the answer is to rebuild
/// rather than to write a transformation from the old shape to the new one —
/// which means no migration code exists, and none ever accumulates.
///
/// The new generation is written *beside* the current one and the pointer moves
/// afterwards. Renaming over an open database looks equivalent and is not:
/// readers keep the file they already opened, and SQLite's sidecar files are
/// named from the path, so a fresh database can end up paired with the previous
/// one's write-ahead log.
pub fn rebuild(paths: StorePaths) -> Result<()> {
    let s = crate::read::session(paths)?;
    let events = s.log.all()?;
    let ops = Ops::open(&s.paths.ops)?;
    let current = ops.generation()?;

    let next = projection::rebuild(
        &s.paths.state,
        &events,
        &s.log.aside_rows()?,
        current,
        crate::read::now_ms(),
    )?;
    ops.set_generation(next)?;

    let store = Store::open_read(&projection::generation_path(&s.paths.state, next))?;
    say!(
        "rebuilt {} event(s) into generation {next} — {} note(s)",
        events.len(),
        store.note_count()?
    );
    Ok(())
}

// ------------------------------------------------------------------ set ----

/// Accept a device this one did not admit.
///
/// Matched by prefix, because the prefix is what `capture health` prints and
/// what a person can compare against the other device's screen. An ambiguous
/// prefix is refused rather than guessed: acknowledging the wrong key would
/// silence the report about a device nobody recognises while leaving the one
/// that was actually expected still shouting.
pub fn admission_seen(paths: StorePaths, key: &str) -> Result<()> {
    let ops = Ops::open(&paths.ops)?;
    let identity = capture_store::identity::Identity::load(&paths)?;
    let own = std::collections::BTreeSet::from([identity.public().to_bytes()]);

    let wanted = key.to_ascii_lowercase();
    let candidates: Vec<[u8; 32]> = ops
        .unexpected_admissions(&roster_now(&paths)?.keys, &own)?
        .into_iter()
        .filter(|k| short_key(k).starts_with(&wanted))
        .collect();

    match candidates.as_slice() {
        [] => {
            say!("No unacknowledged admission matches {key}.");
            Ok(())
        }
        [one] => {
            let now = crate::read::now_ms() as u64;
            ops.acknowledge_admission(one, now)?;
            say!("Acknowledged {}.", short_key(one));
            say!("Recorded as seen, not as paired — this device still did not admit it.");
            Ok(())
        }
        many => {
            say!("{} admissions start with {key}:", many.len());
            for k in many {
                say!("  {}", short_key(k));
            }
            say!("Give more of the key.");
            Ok(())
        }
    }
}

/// Acknowledge that stale export files have been dealt with.
pub fn exports_pruned(paths: StorePaths) -> Result<()> {
    let ops = Ops::open(&paths.ops)?;
    let n = ops.stale_export_count()?;
    if n == 0 {
        say!("No exports are known to be stale.");
        return Ok(());
    }
    ops.exports_pruned()?;
    say!("Acknowledged. {n} redaction(s) will stop being reported.");
    say!("Nothing was checked — this records your word for it.");
    Ok(())
}

/// Record a preference.
pub fn set(paths: StorePaths, key: &str, value: Option<&str>) -> Result<()> {
    // The store validates the key and writes the event. This used to do both,
    // which left `mutate::set_setting` unused and unchecked — so a second
    // surface calling it could have written a key no build understands into a
    // log that keeps it for ever.
    capture_store::mutate::set_setting(&paths, key, value)?;
    match value {
        Some(v) => say!("{key} set to {v}"),
        None => say!("{key} restored to its default"),
    }
    Ok(())
}

// ----------------------------------------------------------------- utils ----

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn when(ms: u64) -> String {
    jiff::Timestamp::from_millisecond(ms as i64)
        .map(|t| t.strftime("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_else(|_| "unknown".into())
}

fn ms(v: u64) -> String {
    let secs = v / 1000;
    match secs {
        0..=59 => format!("{secs}s"),
        60..=3599 => format!("{}m{}s", secs / 60, secs % 60),
        _ => format!("{}h{}m", secs / 3600, (secs % 3600) / 60),
    }
}

/// Enough of a key for a person to compare against another device's screen.
///
/// The length is `capture_core::id::SHOWN_KEY_BYTES`, decided once. Seven
/// surfaces used to decide it separately, which is a disagreement waiting to
/// happen at the moment somebody is comparing two screens.
fn short_key(k: &[u8; 32]) -> String {
    capture_core::id::short_key(k)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_known_setting_has_a_default_or_says_it_has_none() {
        // The compiled-in value is what makes "never chosen" a usable state
        // rather than an absence every caller has to handle — except where the
        // unchosen case is genuinely a rule, and then the absence is the point.
        for s in settings::KNOWN {
            // Either a value, or an explicit absence meaning "a rule decides".
            // What is not allowed is an empty string, which reads as a value
            // and behaves as neither.
            assert!(
                s.default != Some(""),
                "{} has an empty default, which is neither a value nor a rule",
                s.key
            );
            assert!(!s.what.is_empty(), "{} has no description", s.key);
        }
        assert!(
            !settings::KNOWN.is_empty(),
            "there are no settings, so this checks nothing"
        );
    }

    #[test]
    fn an_unknown_setting_has_no_default() {
        assert_eq!(settings::default_for("nonsense.key"), None);
    }

    #[test]
    fn known_kinds_are_named_rather_than_numbered() {
        assert_eq!(kind_name(Kind::CAPTURE), "capture");
        assert_eq!(kind_name(Kind::ALIAS), "alias");
        assert_eq!(kind_name(Kind::ENTITY_RENAME), "entity-rename");
        assert_eq!(kind_name(Kind::ENTITY_MERGE), "entity-merge");
        assert_eq!(kind_name(Kind::ENTITY_BIND), "entity-bind");
    }

    #[test]
    fn every_kind_this_build_knows_has_a_name() {
        // The gap this closes was silent and wide: fifteen kinds this build
        // writes itself — `promote`, `intent`, `review`, every `occ_*`,
        // `parent`, both `item_*`, the roster kinds — fell through to
        // "unknown", so `capture log` blamed a newer client for events it had
        // authored.
        //
        // Over the range rather than a list, so a kind added later fails here
        // instead of quietly joining them.
        for n in 1..=32u16 {
            let k = Kind(n);
            assert!(k.is_known(), "Kind({n}) should be known to this build");
            assert_ne!(
                kind_name(k),
                "unknown",
                "Kind({n}) is known but has no name"
            );
        }
    }

    #[test]
    fn an_unrecognised_kind_is_named_as_unknown() {
        // Not blank, and not an error: almost always a newer client, which is a
        // different fact from a corrupt event and must read as one.
        assert_eq!(kind_name(Kind(9999)), "unknown");
    }
}
