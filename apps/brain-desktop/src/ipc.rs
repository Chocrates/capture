//! What the webview may ask for.
//!
//! # The whole boundary is here
//!
//! Everything the interface can do is a function in this file. There is no
//! server, no port, and no way for the page to reach the store except through
//! one of these — which is the point of writing them out rather than exposing a
//! query interface: the surface is enumerable, and adding to it is a deliberate
//! act with a diff.
//!
//! # Reads are read-only, writes take the lock
//!
//! Read commands open the projection read-only and hold nothing, so the window
//! keeps working while the command line or the overlay is writing. Write
//! commands open a session, which takes the writer lock for the length of the
//! call and releases it — never for the lifetime of the window, or the desktop
//! app being open would stop every other binary from working.
//!
//! # Errors cross as strings
//!
//! The webview cannot do anything useful with a typed error, and the page has
//! no business branching on error kinds — it shows what went wrong. Exit codes
//! matter at a process boundary, and this is not one.

use brain_store::{
    Error, Result, StorePaths,
    ops::Ops,
    projection::{self, NoteRow, Store},
    session::Session,
};
use serde::Serialize;

/// A note, as the interface needs it.
///
/// Deliberately not the storage type. This is a wire format the page depends
/// on, and letting a storage field rename ripple into the interface is how a
/// refactor becomes a broken window.
#[derive(Serialize)]
pub struct Note {
    pub slug: String,
    pub alias: Option<String>,
    pub text: Option<String>,
    pub captured_ms: u64,
    pub updated_ms: u64,
    pub tz: Option<String>,
    pub tags: Vec<String>,
    pub pinned: bool,
    pub muted: bool,
    pub redacted: bool,
    pub redacted_ms: Option<u64>,
    pub divergences: u64,
    /// Which of the four states the row renders as.
    ///
    /// Decided here rather than in the page, so every surface agrees about what
    /// a note *is* — and so a state added later cannot be silently missed by a
    /// template that only knew about four.
    pub state: &'static str,
}

impl From<NoteRow> for Note {
    fn from(n: NoteRow) -> Note {
        let state = if n.redacted_at.is_some() {
            "redacted"
        } else if n.redaction_queued {
            // Ordered and not yet carried out. The content is already withheld
            // — that is not delayed — but it is still cancellable, and a reader
            // told "destroyed" has been told something they can no longer act
            // on when in fact they can.
            "withheld"
        } else if n.text.is_none() {
            // Not the same as redacted. A note whose content has not arrived is
            // a different fact from one whose content was destroyed, and a
            // reader shown the wrong one has been told the wrong story about
            // their own archive.
            "incomplete"
        } else if n.divergences > 0 {
            "diverged"
        } else if n.muted_until.is_some() {
            "muted"
        } else {
            "ordinary"
        };

        Note {
            slug: n.slug,
            alias: n.alias,
            text: n.text,
            captured_ms: n.created.0,
            updated_ms: n.updated.0,
            tz: n.tz,
            tags: n.tags,
            pinned: n.pinned,
            muted: n.muted_until.is_some(),
            redacted: n.redacted_at.is_some(),
            redacted_ms: n.redacted_at,
            divergences: n.divergences,
            state,
        }
    }
}

/// A link, with enough to render it broken.
#[derive(Serialize)]
pub struct Link {
    pub name: String,
    /// What the name meant when it was written, if that note still exists.
    pub target: Option<String>,
    pub broken: bool,
}

/// A note that points here.
#[derive(Serialize)]
pub struct Backlink {
    pub slug: String,
    /// The name it used, which may not be this note's current one.
    pub name: String,
}

/// Everything the note page shows.
#[derive(Serialize)]
pub struct Page {
    pub note: Note,
    pub links: Vec<Link>,
    pub backlinks: Vec<Backlink>,
    pub versions: u64,
}

/// One thing worth telling the user.
#[derive(Serialize)]
pub struct Condition {
    /// `attention` or `worth-knowing`. Tiered so that a surface full of
    /// undifferentiated warnings does not train the reader to ignore all of
    /// them — most of what is reported here is the system working.
    pub tier: &'static str,
    pub text: String,
    /// Whether the user can dismiss it. True only for conditions this program
    /// cannot verify have been dealt with.
    pub acknowledgeable: bool,
}

/// The health surface.
#[derive(Serialize)]
pub struct Health {
    pub notes: u64,
    pub events: u64,
    pub conditions: Vec<Condition>,
    /// Said out loud rather than left as an empty list: an empty surface is
    /// ambiguous between "nothing is wrong" and "nothing was checked".
    pub healthy: bool,
    /// What this surface did *not* check, named so it cannot be mistaken for a
    /// verification.
    pub unchecked: String,
}

/// A redaction waiting for its window to elapse.
#[derive(Serialize)]
pub struct Pending {
    pub id: String,
    pub slug: String,
    pub execute_after_ms: u64,
}

fn paths() -> Result<StorePaths> {
    StorePaths::resolve()
        .ok_or_else(|| Error::Usage("cannot work out where the store lives".into()))
}

/// Open the projection without taking the writer lock.
fn read_only() -> Result<Store> {
    let p = paths()?;
    let generation = Ops::open_read(&p.ops)?.generation()?;
    Store::open_read(&projection::generation_path(&p.state, generation))
}

fn wire<T>(r: Result<T>) -> std::result::Result<T, String> {
    r.map_err(|e| e.to_string())
}

// ------------------------------------------------------------------ reads ----

#[tauri::command]
pub fn timeline(limit: usize) -> std::result::Result<Vec<Note>, String> {
    wire((|| {
        let s = read_only()?;
        Ok(s.timeline(limit)?.into_iter().map(Note::from).collect())
    })())
}

#[tauri::command]
pub fn note(name: String) -> std::result::Result<Page, String> {
    wire((|| {
        let s = read_only()?;
        let Some(tgt) = s.resolve(&name)? else {
            return Err(Error::NotFound(name.clone()));
        };
        let row = s.note(&tgt)?.ok_or_else(|| Error::NotFound(name.clone()))?;

        let links = s
            .links_from(&tgt)?
            .into_iter()
            .map(|l| Link {
                broken: l.is_broken(),
                target: l.target_slug,
                name: l.name,
            })
            .collect();

        let backlinks = s
            .links_to(&tgt)?
            .into_iter()
            .map(|b| Backlink {
                slug: b.slug,
                name: b.name,
            })
            .collect();

        let versions = row.divergences + 1;
        Ok(Page {
            note: Note::from(row),
            links,
            backlinks,
            versions,
        })
    })())
}

#[tauri::command]
pub fn search(query: String) -> std::result::Result<Vec<Note>, String> {
    wire((|| {
        let s = read_only()?;
        let now = now_ms();
        let mut out = Vec::new();
        for tgt in s.search(&query, now)? {
            if let Some(n) = s.note(&tgt)? {
                out.push(Note::from(n));
            }
        }
        Ok(out)
    })())
}

/// What was going on when a note was captured.
#[tauri::command]
pub fn ambient(name: String) -> std::result::Result<Vec<(String, String)>, String> {
    wire((|| {
        let s = read_only()?;
        let Some(tgt) = s.resolve(&name)? else {
            return Err(Error::NotFound(name.clone()));
        };
        s.ambient_of(&tgt)
    })())
}

/// Notes sharing an ambient value, for the context strip's click-to-filter.
#[tauri::command]
pub fn filter_by_context(field: String, value: String) -> std::result::Result<Vec<Note>, String> {
    wire((|| {
        let s = read_only()?;
        Ok(s.by_context(&field, &value)?
            .into_iter()
            .map(Note::from)
            .collect())
    })())
}

/// What a reader has already accounted for.
///
/// Held for the lifetime of the window so that a change is reported once
/// rather than every time it is asked about.
#[derive(Default)]
pub struct Watcher(pub std::sync::Mutex<Option<brain_store::watch::Watch>>);

/// Whether anything has moved since the last look.
///
/// The page asks on a short timer. `watch.rs` argues for polling over being
/// told, and the argument is about failure rather than latency: a reader that
/// waits to be notified shows stale data forever if the message is dropped or
/// the writer died between committing and notifying, while a reader that looks
/// for itself is at worst late. Two integer reads, through read-only
/// connections that do not need the writer to be running.
///
/// Two different things move and both matter. New events mean a capture
/// happened — from the overlay, the terminal chord, or another window. A new
/// generation means the projection was rebuilt underneath us, and the file this
/// window has been reading is one nobody writes to any more: same data, frozen.
#[tauri::command]
pub fn changed(watcher: tauri::State<'_, Watcher>) -> std::result::Result<bool, String> {
    wire((|| {
        let p = paths()?;
        let ops = Ops::open_read(&p.ops)?;
        let log = brain_store::log::Log::open_read(&p.events)?;

        let mut guard = watcher.0.lock().unwrap_or_else(|e| e.into_inner());
        match guard.as_mut() {
            Some(w) => Ok(w.poll(&log, &ops)?.any()),
            None => {
                // The first look establishes where things stand. Reporting a
                // change here would refresh the page immediately after it drew.
                *guard = Some(brain_store::watch::Watch::start(&log, &ops)?);
                Ok(false)
            }
        }
    })())
}

#[tauri::command]
pub fn health() -> std::result::Result<Health, String> {
    wire((|| {
        let p = paths()?;
        let s = read_only()?;
        let ops = Ops::open_read(&p.ops)?;
        let log = brain_store::log::Log::open_read(&p.events)?;
        let now = now_ms();

        let mut conditions = Vec::new();

        // Tier one: things wanting a decision. Authentication failures are
        // never folded into a general total — an event that failed to
        // authenticate is a security signal, and burying it alongside version
        // skew is how it goes unnoticed.
        for (reason, n) in ops.rejected_counts()? {
            conditions.push(Condition {
                tier: "attention",
                text: format!("{n} event(s) failed authentication ({reason})"),
                acknowledgeable: false,
            });
        }
        if brain_store::spool::in_use(&p.spool)? {
            conditions.push(Condition {
                tier: "attention",
                text: "captures are being spooled — the store could not be written".into(),
                acknowledgeable: false,
            });
        }
        // A chord that has quietly stopped working is invisible everywhere
        // else: the desktop still lists the binding and nothing logs an error.
        let mechanism = brain_store::hotkey::select(&brain_store::hotkey::Host);
        let chord = brain_store::hotkey::DEFAULT_CHORD.human();
        if let Some(text) = brain_store::gesture::health(
            mechanism != brain_store::hotkey::Mechanism::None,
            now,
            ops.gesture_last_fired()?,
            ops.last_active()?,
        )
        .message(&chord)
        {
            conditions.push(Condition {
                tier: "attention",
                text,
                acknowledgeable: false,
            });
        }

        let stale = ops.stale_export_count()?;
        if stale > 0 {
            conditions.push(Condition {
                tier: "attention",
                text: format!(
                    "{stale} redaction(s) destroyed content that older export files still \
                     contain. Redaction cannot reach a backup."
                ),
                // The files are somewhere this program cannot look, so it can
                // never verify they were dealt with.
                acknowledgeable: true,
            });
        }

        // Tier two: worth knowing, no action. Described by cause — most of
        // these are a newer client writing kinds this build has no rule for,
        // which is the system working.
        let held = s.held_count()?;
        if held > 0 {
            conditions.push(Condition {
                tier: "worth-knowing",
                text: format!("{held} event(s) stored but not folded — no rule for them here"),
                acknowledgeable: false,
            });
        }
        for (_id, reason) in s.quarantined()? {
            conditions.push(Condition {
                tier: "worth-knowing",
                text: format!("an event was set aside: {reason}"),
                acknowledgeable: false,
            });
        }

        let attention = conditions.iter().filter(|c| c.tier == "attention").count();
        Ok(Health {
            notes: s.note_count()?,
            events: log.count()?,
            healthy: attention == 0,
            conditions,
            unchecked: "Signatures, body hashes and origin chains are not checked here. \
                        Run `brain verify` for that."
                .into(),
        })
    })())
}

#[tauri::command]
pub fn pending_redactions() -> std::result::Result<Vec<Pending>, String> {
    wire((|| {
        let s = read_only()?;
        let mut out = Vec::new();
        for (redact, tgt, execute_after, cancelled) in s.redact_queue()? {
            if cancelled {
                continue;
            }
            let slug = s.note(&tgt)?.map(|n| n.slug).unwrap_or_default();
            out.push(Pending {
                id: redact.0[..4].iter().map(|b| format!("{b:02x}")).collect(),
                slug,
                execute_after_ms: execute_after,
            });
        }
        Ok(out)
    })())
}

// ----------------------------------------------------------------- writes ----
//
// Each opens a session, does one thing, and drops it. Holding the writer lock
// for the lifetime of the window would stop the command line and the overlay
// from working for as long as the desktop app was open.

#[tauri::command]
pub fn edit(name: String, text: String) -> std::result::Result<(), String> {
    wire((|| brain_store::mutate::edit(&paths()?, &name, &text))())
}

#[tauri::command]
pub fn tag(name: String, tag: String) -> std::result::Result<(), String> {
    wire((|| brain_store::mutate::tag(&paths()?, &name, &tag))())
}

#[tauri::command]
pub fn untag(name: String, tag: String) -> std::result::Result<(), String> {
    wire((|| brain_store::mutate::untag(&paths()?, &name, &tag))())
}

#[tauri::command]
pub fn set_alias(name: String, alias: Option<String>) -> std::result::Result<(), String> {
    wire((|| {
        brain_store::mutate::set_alias(&paths()?, &name, alias.as_deref())
    })())
}

#[tauri::command]
pub fn pin(name: String, pinned: bool) -> std::result::Result<(), String> {
    wire((|| brain_store::mutate::pin(&paths()?, &name, pinned))())
}

#[tauri::command]
pub fn mute(name: String) -> std::result::Result<(), String> {
    wire((|| brain_store::mutate::mute_note(&paths()?, &name))())
}

#[tauri::command]
pub fn resolve(name: String) -> std::result::Result<usize, String> {
    // How many competing versions were dismissed, so the interface can say so
    // rather than implying one was chosen. Both remain in the log.
    wire((|| brain_store::mutate::resolve(&paths()?, &name))())
}

/// Queue a redaction.
///
/// The confirmation is the interface's job, not this function's — but the
/// window it opens is not: the delay, the cap and the snapshot are properties
/// of the operation and apply however it was reached.
#[tauri::command]
pub fn redact(name: String) -> std::result::Result<String, String> {
    wire((|| brain_store::mutate::redact(&paths()?, &name))())
}

#[tauri::command]
pub fn cancel_redaction(id: String) -> std::result::Result<(), String> {
    wire((|| brain_store::mutate::cancel_redaction(&paths()?, &id))())
}

#[tauri::command]
pub fn acknowledge_exports() -> std::result::Result<(), String> {
    wire((|| {
        Ops::open(&paths()?.ops)?.exports_pruned()?;
        Ok(())
    })())
}

/// Drain anything that has become due.
///
/// Called on a slow tick from the page. This is the latency half of the
/// due-work rule — every process startup is the floor that makes the guarantee,
/// and a resident window is what makes a deadline land near its time.
#[tauri::command]
pub fn tick() -> std::result::Result<usize, String> {
    wire((|| {
        let mut s = Session::open(paths()?)?;
        Ok(s.drain_due()?.destroyed)
    })())
}

fn now_ms() -> u64 {
    jiff::Timestamp::now().as_millisecond().max(0) as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use brain_store::projection::NoteRow;

    fn row() -> NoteRow {
        NoteRow {
            tgt: brain_core::EventId([1; 32]),
            slug: "a-note-here".into(),
            alias: None,
            text: Some("some text".into()),
            created: (1_700_000_000_000, 0),
            updated: (1_700_000_000_000, 0),
            tz: Some("UTC".into()),
            pinned: false,
            muted_until: None,
            redacted_at: None,
            redaction_queued: false,
            credential: false,
            tags: Vec::new(),
            divergences: 0,
        }
    }

    #[test]
    fn an_ordinary_note_says_so() {
        assert_eq!(Note::from(row()).state, "ordinary");
    }

    #[test]
    fn withheld_outranks_incomplete_and_is_not_destroyed() {
        // A note queued for redaction has no readable text, which is exactly
        // what "incomplete" also looks like from the row alone. Telling them
        // apart is the point: one is content that has not arrived, the other is
        // content being withheld on the way to destruction — and only the
        // second is something the reader can still cancel.
        let mut queued = row();
        queued.text = None;
        queued.redaction_queued = true;
        assert_eq!(Note::from(queued).state, "withheld");

        // Destruction still wins. Once the body is gone, "queued" is history.
        let mut both = row();
        both.text = None;
        both.redaction_queued = true;
        both.redacted_at = Some(1);
        assert_eq!(Note::from(both).state, "redacted");
    }

    #[test]
    fn destroyed_and_incomplete_are_different_states() {
        // The distinction the interface most needs to get right. A note whose
        // content has not arrived and one whose content was destroyed both have
        // no text, and telling a reader the wrong one is telling them the wrong
        // story about their own archive.
        let mut destroyed = row();
        destroyed.text = None;
        destroyed.redacted_at = Some(1_700_000_100_000);
        assert_eq!(Note::from(destroyed).state, "redacted");

        let mut withheld = row();
        withheld.text = None;
        withheld.redaction_queued = true;

        let mut incomplete = row();
        incomplete.text = None;
        assert_eq!(Note::from(incomplete).state, "incomplete");
    }

    #[test]
    fn a_diverged_note_reports_its_versions() {
        let mut n = row();
        n.divergences = 2;
        let wire = Note::from(n);
        assert_eq!(wire.state, "diverged");
        assert_eq!(wire.divergences, 2);
    }

    #[test]
    fn a_muted_note_is_still_a_note() {
        // It stays in the timeline with a marker. Muting suppresses it
        // elsewhere; a mute that removed it from the one place it can be seen
        // in context would be a deletion with a friendlier name.
        let mut n = row();
        n.muted_until = Some(0);
        let wire = Note::from(n);
        assert_eq!(wire.state, "muted");
        assert!(wire.text.is_some());
    }

    #[test]
    fn destruction_outranks_every_other_state() {
        // A destroyed note that was also muted and diverged is reported as
        // destroyed. The others describe how it is shown; this one describes
        // what happened to it.
        let mut n = row();
        n.text = None;
        n.redacted_at = Some(1_700_000_100_000);
        n.muted_until = Some(0);
        n.divergences = 3;
        assert_eq!(Note::from(n).state, "redacted");
    }

    #[test]
    fn every_state_has_a_name() {
        // A state that fell through to nothing would render as a blank marker,
        // indistinguishable from a rendering fault. Each branch is reached.
        let mut destroyed = row();
        destroyed.text = None;
        destroyed.redacted_at = Some(1);

        let mut withheld = row();
        withheld.text = None;
        withheld.redaction_queued = true;

        let mut incomplete = row();
        incomplete.text = None;

        let mut diverged = row();
        diverged.divergences = 1;

        let mut muted = row();
        muted.muted_until = Some(0);

        let named: Vec<&str> = [row(), destroyed, withheld, incomplete, diverged, muted]
            .into_iter()
            .map(|n| Note::from(n).state)
            .collect();
        assert!(named.iter().all(|s| !s.is_empty()));
        assert_eq!(
            named,
            [
                "ordinary",
                "redacted",
                "withheld",
                "incomplete",
                "diverged",
                "muted"
            ]
        );
    }
}
