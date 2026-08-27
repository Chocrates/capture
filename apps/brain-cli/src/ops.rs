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
//!   admitting the gap. So it names `brain verify` rather than implying it.
//!
//! # Nothing here scores the user
//!
//! Counts describe the data — how many notes, how many quarantined events.
//! Nothing describes the person: no streak, no capture target, no completion
//! figure. Repeated performance failure is a documented mechanism behind the
//! anxiety this design exists for, so a status page that manufactures a running
//! failure signal works directly against its own purpose.

use brain_core::{body::SettingBody, event::Kind};
use brain_store::{
    Error, Result, StorePaths,
    lock::{self, Writer},
    log::Log,
    ops::Ops,
    projection::{self, Store},
    store,
};

use crate::read;
use brain_store::session::Session;

/// Preferences this build understands, with the value used when none is chosen.
///
/// Compiled in, never written to the log on first run. Materialising defaults
/// as recorded decisions would freeze today's judgement permanently and destroy
/// the distinction between a value you picked and one you inherited.
pub const KNOWN_SETTINGS: &[(&str, &str, &str)] = &[(
    "timeline.limit",
    "50",
    "how many notes `brain timeline` shows",
)];

fn default_for(key: &str) -> Option<&'static str> {
    KNOWN_SETTINGS
        .iter()
        .find(|(k, _, _)| *k == key)
        .map(|(_, v, _)| *v)
}

/// The value in force for a setting, chosen or compiled in.
pub fn effective(state: &Store, key: &str) -> Result<Option<String>> {
    for (k, v, _) in state.settings()? {
        if k == key {
            // An explicit revert is stored as a choice with no value, and takes
            // the default — which is why this falls through rather than
            // returning `None` as "unset".
            return Ok(v.or_else(|| default_for(key).map(String::from)));
        }
    }
    Ok(default_for(key).map(String::from))
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

    if json {
        let rows: Vec<String> = events
            .iter()
            .map(|e| {
                format!(
                    "{{\"id\":\"{}\",\"kind\":\"{}\",\"seq\":{},\"origin\":\"{}\",\"wall_ms\":{}}}",
                    hex(&e.header.id().0[..8]),
                    kind_name(e.header.kind),
                    e.header.seq,
                    hex(&e.header.dev.short()),
                    e.header.hlc.wall
                )
            })
            .collect();
        say!("[{}]", rows.join(","));
        return Ok(());
    }

    say!("{name}");
    for e in &events {
        say!(
            "  {}  {:<14} {} seq {}  {}",
            hex(&e.header.id().0[..4]),
            kind_name(e.header.kind),
            hex(&e.header.dev.short()),
            e.header.seq,
            when(e.header.hlc.wall)
        );
    }
    Ok(())
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
        // A kind this build has never heard of. Named as such rather than
        // rendered blank: it is almost certainly a newer client, which is a
        // different fact from a corrupt event.
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
    let mechanism = brain_store::hotkey::select(&brain_store::hotkey::Host);
    let chord = brain_store::hotkey::DEFAULT_CHORD.human();
    say!("chord       {chord} via {}", mechanism.name());

    // A default is a rule; a setting is a decision. §8.9 keeps them apart, and
    // this is the line where the difference becomes visible: a machine-chosen
    // notifier reported as "set by you" is a preference nobody made that can
    // never be centrally improved.
    match brain_store::notifier::chosen(&state)? {
        Some(who) => say!("notifier    {who} — your choice"),
        None => say!(
            "notifier    {} — a default, not a setting",
            brain_store::notifier::default_rule()
        ),
    }

    // ---- tier one: things that want a decision -----------------------------
    let mut attention: Vec<String> = Vec::new();

    let health = brain_store::gesture::health(
        mechanism != brain_store::hotkey::Mechanism::None,
        jiff::Timestamp::now().as_millisecond().max(0) as u64,
        ops.gesture_last_fired()?,
        ops.last_active()?,
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
    let now = jiff::Timestamp::now().as_millisecond().max(0) as u64;
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
             {}, then: brain exports-pruned",
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
    if brain_store::spool::in_use(&paths.spool)? {
        attention.push("captures are being spooled — the store could not be written".into());
    }
    let identity = brain_store::identity::Identity::load(paths)?;
    let w = identity.watermark();
    let lead = brain_core::hlc::HlcState::restore(w.hlc_wall, w.hlc_counter)
        .lead_ms(jiff::Timestamp::now().as_millisecond().max(0) as u64);
    if lead > brain_core::hlc::LEAD_REPORT_MS {
        attention.push(format!(
            "this device's clock reads {} behind the events it is writing — \
             ordering is distorted until it is corrected (captures still work)",
            ms(lead)
        ));
    }

    // ---- tier two: worth knowing, no action ---------------------------------
    let mut worth_knowing: Vec<String> = Vec::new();
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
        worth_knowing.push(format!(
            "{held} event(s) stored but not folded — this build has no rule for them"
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
    for (key, default, what) in KNOWN_SETTINGS {
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
    say!("  brain verify");
    Ok(())
}

// ----------------------------------------------------- quarantine, rejected ----

/// Events that are authentic but this device cannot use.
pub fn quarantine(paths: &StorePaths, json: bool) -> Result<()> {
    let state = read::open(paths)?;
    let rows = state.quarantined()?;

    if json {
        let out: Vec<String> = rows
            .iter()
            .map(|(id, reason)| {
                format!("{{\"id\":\"{}\",\"reason\":\"{reason}\"}}", hex(&id.0[..8]))
            })
            .collect();
        say!("[{}]", out.join(","));
        return Ok(());
    }

    if rows.is_empty() {
        say!("Nothing is quarantined.");
        return Ok(());
    }
    say!("Set aside — authentic, and holding their positions, but unusable here.");
    say!();
    for (id, reason) in &rows {
        say!("  {}  {reason}", hex(&id.0[..8]));
    }
    Ok(())
}

/// Events that failed authentication.
pub fn rejected(paths: &StorePaths, json: bool) -> Result<()> {
    let ops = Ops::open_read(&paths.ops)?;
    let counts = ops.rejected_counts()?;

    if json {
        let out: Vec<String> = counts
            .iter()
            .map(|(reason, n)| format!("{{\"reason\":\"{reason}\",\"count\":{n}}}"))
            .collect();
        say!("[{}]", out.join(","));
        return Ok(());
    }

    if counts.is_empty() {
        say!("No event has failed authentication.");
        return Ok(());
    }
    say!("Refused — not authenticated, holding no position in any sequence.");
    say!();
    for (reason, n) in &counts {
        say!("  {n:<6} {reason}");
    }
    Ok(())
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
    let s = Session::open(paths)?;
    let events = s.log.all()?;
    let ops = Ops::open(&s.paths.ops)?;
    let current = ops.generation()?;

    let next = projection::rebuild(
        &s.paths.state,
        &events,
        current,
        jiff::Timestamp::now().as_millisecond().max(0),
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
    if default_for(key).is_none() {
        let known: Vec<&str> = KNOWN_SETTINGS.iter().map(|(k, _, _)| *k).collect();
        return Err(Error::Usage(format!(
            "'{key}' is not a setting this build knows.\n\nKnown: {}",
            known.join(", ")
        )));
    }
    let mut s = Session::open(paths)?;
    s.append(Kind::SETTING, None, None, |nonce| {
        let mut b = Vec::new();
        minicbor::encode(
            &SettingBody {
                nonce,
                key: key.to_string(),
                value: value.map(String::from),
            },
            &mut b,
        )
        .expect("encoding into Vec is infallible");
        b
    })?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_known_setting_has_a_default() {
        // The compiled-in value is what makes "never chosen" a usable state
        // rather than an absence the caller has to handle.
        for (k, v, what) in KNOWN_SETTINGS {
            assert!(!v.is_empty(), "{k} has no default");
            assert!(!what.is_empty(), "{k} has no description");
        }
    }

    #[test]
    fn an_unknown_setting_has_no_default() {
        assert_eq!(default_for("nonsense.key"), None);
    }

    #[test]
    fn known_kinds_are_named_rather_than_numbered() {
        assert_eq!(kind_name(Kind::CAPTURE), "capture");
        assert_eq!(kind_name(Kind::ALIAS), "alias");
    }

    #[test]
    fn an_unrecognised_kind_is_named_as_unknown() {
        // Not blank, and not an error: almost always a newer client, which is a
        // different fact from a corrupt event and must read as one.
        assert_eq!(kind_name(Kind(9999)), "unknown");
    }
}
