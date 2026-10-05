//! Changing a note: edit, tag, untag, alias, pin, mute, resolve.
//!
//! # Nothing here overwrites anything
//!
//! Every one of these appends an event. An edit does not replace the old text,
//! it records a new version alongside it; an untag names the specific additions
//! it retracts; unpinning is a pin event carrying `false`. The projection is
//! recomputed from the result.
//!
//! That is the whole reason the archive can be trusted: there is no operation
//! in this file that can lose something, so no bug in it can either. The only
//! destructive operation in the system is redaction, which lives elsewhere and
//! is deliberately harder to reach.
//!
//! # A name means one thing
//!
//! `alias` shares a single namespace with slugs, so an alias can never shadow
//! another note's canonical name. The refusal happens twice on purpose: here,
//! where it can explain itself to a person, and again in the fold, which is
//! what makes every replica decline the same assignment even if two devices
//! claim the name while partitioned.

use capture_core::{
    EventId,
    body::{
        AliasBody, EditBody, MuteScope, PinBody, RedactCancelBody, ResolveBody, TagBody, UntagBody,
    },
    event::Kind,
};
use capture_store::{Error, Result, StorePaths};

use capture_store::session::Session;

// One home for what a sigil names; see `read::split_mention`.
use crate::read::split_mention;
use crate::read::{now_ms, quote};

/// Resolve a name against the projection, or say it is not there.
fn find(s: &Session, name: &str) -> Result<EventId> {
    s.state
        .resolve(name)?
        .ok_or_else(|| Error::NotFound(name.to_string()))
}

/// Replace a note's text with a new version.
///
/// The previous version is not discarded — it stays in the log and in the
/// version list, which is what lets a divergence show both sides rather than
/// picking one and hoping.
pub fn edit(paths: StorePaths, name: &str, text: &str) -> Result<()> {
    if text.trim().is_empty() {
        return Err(Error::Usage(
            "an edit with no text would leave the note unreadable; redact it if that is what you \
             mean"
                .into(),
        ));
    }
    let mut s = crate::read::session(paths)?;
    let tgt = find(&s, name)?;

    // The version being edited, not the note.
    //
    // This read `s.state.note(&tgt)?.map(|n| n.tgt)` — a variable named `head`
    // holding the note's *root*, because `NoteRow` has no head field and
    // `n.tgt` is what was there to take. `resolve` calls a version a tip when
    // nobody names it as `prev`, so every edit after the first was a tip:
    // three sequential edits on one device, a minute apart, came back as
    // "3 independent versions, none superseded".
    //
    // A conflict signal that fires when nothing conflicts is one somebody
    // learns to dismiss by the second day, and then it is not a signal — which
    // matters because §6 rates a genuinely lost edit as the failure worth
    // avoiding, and this is what was supposed to catch it.
    let head = s.state.head_of(&tgt)?.or(Some(tgt));

    s.append(Kind::EDIT, Some(tgt), head, |nonce| {
        encode(&EditBody {
            nonce,
            text: text.to_string(),
        })
    })?;
    say!("{name} edited");
    Ok(())
}

/// Add a tag.
pub fn tag(paths: StorePaths, name: &str, tag: &str) -> Result<()> {
    let tag = tag.trim_start_matches('#');
    if tag.is_empty() {
        return Err(Error::Usage("a tag needs a name".into()));
    }
    let mut s = crate::read::session(paths)?;
    let tgt = find(&s, name)?;
    s.append(Kind::TAG, Some(tgt), None, |nonce| {
        encode(&TagBody {
            nonce,
            tag: tag.to_string(),
        })
    })?;
    say!("{name} tagged #{tag}");
    Ok(())
}

/// Remove a tag.
///
/// The event names the specific additions it retracts, rather than saying "this
/// tag is gone". Tags are an add-wins set: a tag added on another device that
/// this one has not seen must survive a removal that never saw it, and that is
/// only expressible by naming what was actually observed.
pub fn untag(paths: StorePaths, name: &str, tag: &str) -> Result<()> {
    let tag = tag.trim_start_matches('#');
    let mut s = crate::read::session(paths)?;
    let tgt = find(&s, name)?;

    let kills = s.state.tag_events(&tgt, tag)?;
    if kills.is_empty() {
        return Err(Error::NotFound(format!("{name} is not tagged #{tag}")));
    }
    s.append(Kind::UNTAG, Some(tgt), None, |nonce| {
        encode(&UntagBody {
            nonce,
            kills: kills.clone(),
        })
    })?;
    say!("{name} untagged #{tag}");
    Ok(())
}

/// Give a note a chosen name, or clear it.
pub fn alias(paths: StorePaths, name: &str, new: Option<&str>) -> Result<()> {
    let mut s = crate::read::session(paths)?;
    let tgt = find(&s, name)?;

    if let Some(new) = new {
        if new.trim().is_empty() {
            return Err(Error::Usage("an alias needs a name".into()));
        }
        // Checked here so a person gets a reason, and again in the fold so two
        // devices claiming the name while partitioned agree on who keeps it.
        if let Some(holder) = s.state.resolve(new)?
            && holder != tgt
        {
            let held_by = s
                .state
                .note(&holder)?
                .map(|n| n.slug)
                .unwrap_or_else(|| "another note".into());
            return Err(Error::Usage(format!(
                "'{new}' already names {held_by}.\n\
                 \n\
                 Names are shared between slugs and aliases, so one name always \
                 means one note — otherwise a link written today could point \
                 somewhere else tomorrow."
            )));
        }
    }

    s.append(Kind::ALIAS, Some(tgt), None, |nonce| {
        encode(&AliasBody {
            nonce,
            alias: new.map(|a| a.to_string()),
        })
    })?;
    match new {
        Some(a) => say!("{name} is now also {a}"),
        None => say!("{name} alias cleared"),
    }
    Ok(())
}

/// Pin a note, or unpin it.
pub fn pin(paths: StorePaths, name: &str, pinned: bool) -> Result<()> {
    let mut s = crate::read::session(paths)?;
    let tgt = find(&s, name)?;
    s.append(Kind::PIN, Some(tgt), None, |nonce| {
        encode(&PinBody { nonce, pinned })
    })?;
    say!("{name} {}", if pinned { "pinned" } else { "unpinned" });
    Ok(())
}

/// Mark one occurrence of a recurring series complete, or not.
///
/// Named by its original start, which is what an occurrence *is* (§3.5) — the
/// instant the rule would have produced, before any move. A moved occurrence
/// keeps the name, so completing one that somebody rescheduled still finds it.
///
/// Un-completing is a separate kind rather than a toggle, because it has to be
/// possible to undo a mistaken tick and a pure union could not express that.
pub fn occurrence(paths: StorePaths, name: &str, occ: i64, done: bool) -> Result<()> {
    let mut s = crate::read::session(paths)?;
    let tgt = find(&s, name)?;
    let kind = if done {
        Kind::OCC_DONE
    } else {
        Kind::OCC_UNDONE
    };

    s.append(kind, Some(tgt), None, |nonce| {
        encode(&capture_core::body::OccBody { nonce, occ })
    })?;

    say!(
        "{name} {} for that occurrence",
        if done {
            "complete"
        } else {
            "no longer complete"
        }
    );
    Ok(())
}

/// Skip one occurrence of a series, or put it back.
///
/// The store decides both, because §8.4 wants every mutation reachable from the
/// window as well and a second implementation there is how two surfaces come to
/// disagree. `occurrence` above still writes its own event and should probably
/// move too; that is a separate tidy rather than something to do halfway here.
/// Move one occurrence, leaving the rule and its siblings alone.
///
/// Says what was *not* changed as well as what was, because that is the whole
/// distinction §3.5 draws between the three edit scopes — and somebody reaching
/// for this may have meant to change the schedule.
pub fn move_occurrence(
    paths: &StorePaths,
    name: &str,
    occ: i64,
    start_ms: i64,
    end_ms: Option<i64>,
) -> Result<()> {
    capture_store::mutate::move_occurrence(paths, name, occ, start_ms, end_ms)?;
    say!("{name} moved for that occurrence.");
    say!("The rule is unchanged, and every other occurrence stands.");
    Ok(())
}

/// Give one occurrence its own details.
pub fn amend_occurrence(
    paths: &StorePaths,
    name: &str,
    occ: i64,
    title: Option<String>,
    location: Option<String>,
    notes: Option<String>,
) -> Result<()> {
    capture_store::mutate::amend_occurrence(paths, name, occ, title, location, notes)?;
    say!("{name} amended for that occurrence.");
    say!("The rule is unchanged, and every other occurrence keeps the series' own.");
    Ok(())
}

pub fn skip(paths: &StorePaths, name: &str, occ: i64, skipped: bool) -> Result<()> {
    if skipped {
        capture_store::mutate::skip_occurrence(paths, name, occ)?;
        say!("{name} skipped for that occurrence.");
        say!("The rule is unchanged, and every other occurrence stands.");
    } else {
        capture_store::mutate::restore_occurrence(paths, name, occ)?;
        say!("{name} restored for that occurrence.");
        // Said because the log is the record and somebody may go looking.
        say!("The cancellation stays in the log; the move supersedes it.");
    }
    Ok(())
}

/// Record what became of a resurfaced note.
///
/// Three outcomes and no fourth. §2.7 withdrew the graded one — there is
/// nothing here that records whether you remembered, because the system does
/// not need to know in order to schedule well, and a permanent record of recall
/// failures is the most harmful thing this design could contain.
pub fn reviewed(paths: StorePaths, name: &str, outcome: &str) -> Result<()> {
    use capture_core::body::Outcome;

    let outcome = match outcome {
        "engaged" => Outcome::Engaged,
        "deferred" => Outcome::Deferred,
        "shown" => Outcome::Shown,
        other => {
            return Err(capture_store::Error::Usage(format!(
                "'{other}' is not an outcome; use engaged, deferred or shown"
            )));
        }
    };

    capture_store::review::record(&paths, name, outcome)?;
    match outcome {
        Outcome::Engaged => say!("Noted."),
        Outcome::Deferred => say!("Pushed away. Twice and it stops being offered."),
        Outcome::Shown => {}
    }
    Ok(())
}

/// Assign the task type by hand.
///
/// The recognizer promotes a task only where a verb and a day agree (§3.1.5),
/// which leaves every other task to be said out loud. Pinned, like every
/// correction, so no later reparse takes it back off.
pub fn task(paths: StorePaths, name: &str, due: Option<&str>) -> Result<()> {
    capture_store::promote::as_task(&paths, name, due)?;
    match due {
        Some(d) => say!("A task, due {d}."),
        None => say!("A task, with no due date."),
    }
    Ok(())
}

/// Mark a task done, or open again.
pub fn task_done(paths: StorePaths, name: &str, done: bool) -> Result<()> {
    capture_store::promote::set_done(&paths, name, done)?;
    if done {
        say!("Done.");
    } else {
        say!("Open again.");
    }
    Ok(())
}

/// Abandon a plan.
///
/// The fold has always known how to do this and nothing could ask for it: an
/// intent, once made, could not be withdrawn from any surface. §2.4 rates being
/// easy to walk away from as part of the case for having intents at all, and a
/// plan you cannot cancel is a plan that fires at you for ever.
///
/// Named by a prefix of its id, the way a commit is. The whole id is thirty-two
/// bytes and nobody is going to type one.
pub fn forget(paths: StorePaths, id: &str) -> Result<()> {
    let store = crate::read::open(&paths)?;
    let want = id.trim().to_ascii_lowercase();
    if want.len() < 4 {
        return Err(Error::Usage(
            "give at least four characters of the id, from `capture intents`".into(),
        ));
    }

    let matches: Vec<capture_store::projection::IntentRow> = store
        .intents()?
        .into_iter()
        .filter(|i| hex(&i.id).starts_with(&want))
        .collect();

    match matches.as_slice() {
        [] => Err(Error::NotFound(format!("no plan whose id starts {want}"))),
        [one] => {
            let id = capture_core::EventId(
                one.id
                    .clone()
                    .try_into()
                    .map_err(|_| Error::Integrity("a plan has a malformed id".into()))?,
            );
            capture_store::mutate::retract_intent(&paths, &id)?;
            say!("Forgotten: {}", one.then_do);
            Ok(())
        }
        // Refused rather than guessed. Retracting the wrong plan is silent —
        // the one you meant keeps firing and the one you did not is gone.
        many => Err(Error::Usage(format!(
            "{} plans start {want}; give more of the id",
            many.len()
        ))),
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Put a page under another in the wiki, or back at the top.
///
/// The one hand-made structure here. Nothing outside the wiki sidebar reads it,
/// which is what keeps §3.7's objection bounded: a page filed wrongly is in the
/// wrong branch of one list, and is still in the timeline, in search, on every
/// entity page naming it and behind every link to it.
pub fn parent(paths: StorePaths, name: &str, under: Option<&str>) -> Result<()> {
    capture_store::mutate::set_parent(&paths, name, under)?;
    match under {
        Some(p) => say!("{name} sits under {p}."),
        None => say!("{name} is back at the top."),
    }
    Ok(())
}

/// Give an entity a name of your choosing.
///
/// The entity is named by a spelling, and what is written is its identity — so
/// the rename holds however many more spellings later reach the same entity.
pub fn entity_rename(paths: StorePaths, name: &str, display: &str) -> Result<()> {
    let (kind, value) = split_mention(name)?;
    capture_store::mutate::rename_entity(&paths, kind, &value, display)?;
    say!("{name} is now shown as {display}.");
    Ok(())
}

/// Say that two entities were one all along.
///
/// Nothing is moved and nothing is deleted: resolution follows a pointer, so
/// both pages become one page holding what either held.
pub fn entity_merge(paths: StorePaths, from: &str, into: &str) -> Result<()> {
    let (from_kind, from_value) = split_mention(from)?;
    let (into_kind, into_value) = split_mention(into)?;
    if from_kind != into_kind {
        return Err(Error::Usage(format!(
            "{from} and {into} are not the same kind of thing; \
             a {from_kind} and a {into_kind} stay separate"
        )));
    }
    capture_store::mutate::merge_entities(&paths, from_kind, &from_value, &into_value)?;
    say!("{from} and {into} are one {from_kind} now.");
    say!();
    say!("Nothing moved. Either name reaches the same page, and every note");
    say!("that mentioned either still says exactly what it said.");
    Ok(())
}

/// Bind the note that *is* an entity, or unbind it.
pub fn entity_bind(paths: StorePaths, name: &str, note: Option<&str>) -> Result<()> {
    let (kind, value) = split_mention(name)?;
    capture_store::mutate::bind_backing(&paths, kind, &value, note)?;
    match note {
        Some(n) => {
            say!("{n} is the note that is {name}.");
            say!();
            // §4.3 puts the binding on the entity rather than on the note for
            // exactly this reason, and saying so here saves somebody checking.
            say!("It is still whatever it already was — a task stays a task.");
        }
        None => say!("{name} is no longer backed by a note."),
    }
    Ok(())
}

/// Say that missing an instance of a series leaves something owed.
///
/// Only ever set by a person. §3.6 is explicit that the recognizer must not
/// infer this: "walk the dog daily" and "take the pill daily" are the same
/// sentence to a parser and opposite answers to somebody living with the
/// consequences, and guessing wrong in the direction of a manufactured backlog
/// is the todo-app failure the default exists to avoid.
///
/// Pinned, like every correction, so no later reparse takes it back off.
pub fn carries_forward(paths: StorePaths, name: &str) -> Result<()> {
    // The rule about what may carry forward lives in the store, because the
    // review pane asks for the same thing and two copies of "only a series can
    // owe you anything" is one copy too many.
    capture_store::promote::carries_forward(&paths, name)?;
    say!("Noted. Missed instances of {name} will stay until they are dealt with.");
    Ok(())
}

/// Make an if-then plan.
///
/// Two cue shapes today, and only two, because a device may only offer what it
/// can actually watch for. §2.4.1: an intent whose cue this machine cannot
/// observe would be registered, reported as working, and permanently silent.
pub fn intent(paths: StorePaths, cue: &str, arg: &str, then: &str) -> Result<()> {
    use capture_core::body::Cue;

    let cue = match cue {
        "at" => {
            let ms = capture_time::FloatingDate::parse(arg)
                .and_then(|d| capture_time::at_local(d, 9, 0, local_zone()))
                .or_else(|| parse_at(arg))
                .ok_or_else(|| {
                    capture_store::Error::Usage(format!(
                        "'{arg}' is not a time; use YYYY-MM-DD or YYYY-MM-DDTHH:MM"
                    ))
                })?;
            Cue::Time(ms)
        }
        "branch" => Cue::GitBranch(arg.to_string()),
        other => {
            return Err(capture_store::Error::Usage(format!(
                "'{other}' is not a cue this device can watch for; use 'at' or 'branch'"
            )));
        }
    };

    capture_store::mutate::add_intent(&paths, cue, then)?;
    say!("Noted. It fires once, here.");
    Ok(())
}

/// `YYYY-MM-DDTHH:MM`, in this machine's zone.
fn parse_at(s: &str) -> Option<i64> {
    let (date, time) = s.split_once('T')?;
    let d = capture_time::FloatingDate::parse(date)?;
    let (h, m) = time.split_once(':')?;
    capture_time::at_local(d, h.parse().ok()?, m.parse().ok()?, local_zone())
}

/// The zone this machine is in.
///
/// One definition, in `capture-store`'s notifier. This copy was the only one
/// of four that validated `$TZ` before trusting it, and that reading is the
/// one that survived — see `capture_store::notifier::local_zone`.
fn local_zone() -> capture_time::Zone {
    capture_time::Zone::new(&capture_store::notifier::local_zone())
        .unwrap_or_else(|| capture_time::Zone::new("UTC").expect("UTC is always present"))
}

/// Suppress a note from surfacing, without destroying it.
///
/// Muting exists so that making something go away is *cheaper* than destroying
/// it. If suppression were harder to reach than redaction, the gradient would
/// point at the irreversible operation.
pub fn mute(paths: StorePaths, what: &Muted, until: Option<u64>) -> Result<()> {
    let (scope, described) = match what {
        Muted::Note(name) => {
            let s = crate::read::session(paths.clone())?;
            let tgt = find(&s, name)?;
            (MuteScope::Note(tgt), name.clone())
        }
        Muted::Tag(tag) => {
            let tag = tag.trim_start_matches('#').to_string();
            (MuteScope::Tag(tag.clone()), format!("#{tag}"))
        }
        Muted::Days(from, to) => (
            MuteScope::DateRange(from.clone(), to.clone()),
            format!("{from} to {to}"),
        ),

        // Through the store, which resolves the name to an entity id first.
        //
        // The id and not the spelling, so the mute survives a rename and a
        // merge — the thing that makes an entity an identity rather than a
        // word. Returns early because this one is already written by the time
        // the id is known, and reaching the shared append below would write it
        // twice.
        Muted::Entity(kind, name) => {
            capture_store::mutate::mute_entity(&paths, kind, name, until)?;
            said(&format!("everything naming {name}"), until);
            return Ok(());
        }
    };

    // The append is the store's. This wrote its own until now, and the store's
    // version — the one any other surface would call — forgot to name the note
    // a note-scoped mute is about, so the mute worked and was missing from
    // `capture log` for that note.
    capture_store::mutate::mute_scope(&paths, scope, until)?;

    said(&described, until);
    Ok(())
}

/// What muting did, and what it did not do.
///
/// Said every time, because muting is easy to read as deletion — and the whole
/// reason it exists is to be the cheap thing somebody reaches for instead. If
/// suppression were harder to reach than redaction, the gradient would point at
/// the irreversible operation.
fn said(described: &str, until: Option<u64>) {
    match until {
        // Rendered, not printed raw. This said "muted until 1791295200000"
        // until now, and nothing caught it because `until` was always `None`:
        // no syntax could set an end, so the only branch anybody ever reached
        // was the other one. Through `read::stamp` rather than a second
        // formatter, so the end reads the same here as on the note's page.
        Some(t) => say!(
            "{described} muted until {}",
            crate::read::stamp(t, Some(&crate::read::local_zone()))
        ),
        None => say!("{described} muted"),
    }
    say!("It stays in the timeline and stops appearing in searches.");
}

/// Lift a mute.
///
/// A `MUTE` whose end is the present instant, rather than a new event kind.
/// §2.6 requires this — "it is reversible from any device, at any time" — and
/// the machinery was already here: mutes are applied in timestamp order with
/// the newest one touching a note winning, and an end that has passed reads as
/// not muted. A second kind would need its own rule for what an unmute means
/// against a coarser mute still in force, and "newest wins" already answers
/// that once.
pub fn unmute(paths: StorePaths, what: &Muted) -> Result<()> {
    let now = now_ms().max(0) as u64;

    let (scope, described) = match what {
        Muted::Note(name) => {
            // Refused where the answer is knowable. A note carries its mute as
            // a field, so "this was not muted" is a true thing to say rather
            // than a guess.
            let tgt = {
                let s = crate::read::session(paths.clone())?;
                let tgt = find(&s, name)?;
                let muted = s.state.note(&tgt)?.and_then(|n| n.muted_until);
                match muted {
                    Some(0) => {}
                    Some(ends) if ends > now => {}
                    _ => {
                        return Err(Error::NotFound(format!("{name} is not muted")));
                    }
                }
                tgt
            };
            (MuteScope::Note(tgt), name.clone())
        }

        // The three coarser scopes are accepted without checking, because the
        // projection cannot answer whether they are in force. A coarse mute is
        // recorded only as its effect on each note it covers, and nothing
        // distinguishes a note suppressed by a tag mute from one suppressed by
        // its own. There was a `mutes` table once and nothing read or wrote it.
        //
        // So these say what was written rather than what changed. Inventing a
        // scope-level record to improve an error message would be the wrong
        // trade.
        Muted::Tag(tag) => {
            let tag = tag.trim_start_matches('#').to_string();
            (MuteScope::Tag(tag.clone()), format!("#{tag}"))
        }
        Muted::Days(from, to) => (
            MuteScope::DateRange(from.clone(), to.clone()),
            format!("{from} to {to}"),
        ),
        Muted::Entity(kind, name) => {
            capture_store::mutate::mute_entity(&paths, kind, name, Some(now))?;
            lifted(&format!("everything naming {name}"));
            return Ok(());
        }
    };

    capture_store::mutate::mute_scope(&paths, scope, Some(now))?;
    lifted(&described);
    Ok(())
}

/// What lifting a mute did.
///
/// Says that nothing was destroyed on the way back either, for the same reason
/// `said` does: the operation has to read as the cheap reversible one it is.
fn lifted(described: &str) {
    say!("{described} is no longer muted.");
    say!("It appears in searches and on its pages again.");
}

/// When a mute written today should stop.
///
/// A day rather than an instant, because that is how somebody says it ("not
/// until Monday") and what §2.6 asks for: "permanently or until a stated
/// date". Resolved to the start of that day in this machine's zone, which is
/// the same conversion an all-day date gets everywhere else.
///
/// A day already gone is refused rather than accepted. Its effect would be
/// identical to `capture unmute`, and two spellings for one operation is the
/// shape `tools/duplicated.py` exists to object to — here at the surface,
/// where it would also read as a typo somebody got away with.
pub fn mute_ends(day: &str) -> Result<u64> {
    let date = capture_time::FloatingDate::parse(day)
        .ok_or_else(|| Error::Usage(format!("'{day}' is not a day; use YYYY-MM-DD")))?;
    let zone = local_zone();
    let at = capture_time::at_local(date, 0, 0, zone)
        .ok_or_else(|| Error::Usage(format!("{day} does not start in this timezone")))?;

    let now = now_ms();
    if at <= now {
        return Err(Error::Usage(format!(
            "{day} has already begun, so a mute until then would end immediately.\n\
             \n\
             To lift a mute now:  capture unmute <name>"
        )));
    }
    Ok(at as u64)
}

/// What a mute applies to.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Muted {
    /// One note.
    Note(String),
    /// Everything carrying a tag, now or later.
    Tag(String),
    /// Everything captured between two days, inclusive.
    Days(String, String),
    /// Everything naming a person or a project, by kind and written name.
    Entity(String, String),
}

/// Dismiss a note's divergence.
///
/// Dismissing is not choosing. Both versions stay in the log and in the version
/// list; what stops is the note being *reported* as diverged. Nothing here can
/// discard the version that was not picked.
pub fn resolve(paths: StorePaths, name: &str) -> Result<()> {
    let mut s = crate::read::session(paths)?;
    let tgt = find(&s, name)?;

    let others = s.state.divergent_versions(&tgt)?;
    if others.is_empty() {
        return Err(Error::NotFound(format!("{name} has no divergence")));
    }
    let n = others.len();
    s.append(Kind::RESOLVE, Some(tgt), None, |nonce| {
        encode(&ResolveBody {
            nonce,
            dismiss: others.clone(),
        })
    })?;
    say!("{name}: dismissed {n} competing version(s); both remain in the log");
    Ok(())
}

/// Clear a note's credential flag.
///
/// One direction only, and deliberately: the predicate that sets the flag
/// over-flags on purpose, so a person saying "this one is fine" has to be able
/// to override it — but nothing should be able to re-flag a note somebody has
/// already cleared, or it would drop out of backups again on the next rebuild.
pub fn unflag(paths: StorePaths, name: &str) -> Result<()> {
    // The store decides, this says so. There were two implementations of this
    // — one here and one in `mutate` that nothing called — and they agreed
    // only by having been written from each other.
    capture_store::mutate::unflag(&paths, name)?;
    say!("{name} will be included in the next export");
    Ok(())
}

/// Destroy a note's content, after a window in which it can be stopped.
///
/// The only irreversible operation here, and the only one that prompts. The
/// prompt names what will go, individually — a count is dismissible in a way a
/// list of names is not, and this is the one confirmation in the system that
/// must not be trained away by appearing routinely.
pub fn redact(paths: StorePaths, name: &str, force: bool, with_links: bool) -> Result<()> {
    let mut s = crate::read::session(paths)?;
    let tgt = find(&s, name)?;
    let note = s
        .state
        .note(&tgt)?
        .ok_or_else(|| Error::NotFound(name.to_string()))?;

    if note.is_redacted() {
        return Err(Error::Usage(format!("{name} has already been destroyed")));
    }

    // The notes that point here, one hop.
    //
    // §1.1 limit 5: a note's slug is derived from its opening words, and a note
    // linking to it carries that slug inside its own verbatim text. Destroying
    // the target re-derives its name from its id so the name stops carrying the
    // words — and the copies already written into other notes are untouched.
    // §1.1 and §4.3 both name `redact --with-links` as the mitigation and
    // neither one existed.
    //
    // Destroyed, not rewritten. §3.1.3 keeps captured text verbatim for ever,
    // and editing somebody's note to remove a word that is there because of a
    // *different* note's name is the system deciding what they meant.
    //
    // One hop only. A note linking to a note linking to here copied the middle
    // note's name, not this one's, and following further lets one flag walk an
    // arbitrary distance through the archive.
    let mut targets = vec![tgt];
    let linking = if with_links {
        let back = s.state.links_to(&tgt)?;
        for b in &back {
            if !targets.contains(&b.tgt) {
                targets.push(b.tgt);
            }
        }
        back
    } else {
        Vec::new()
    };

    if targets.len() > capture_core::body::REDACT_TARGET_CAP {
        // Refused rather than trimmed. Redacting the ones that fit would leave
        // the rest holding the words the whole operation exists to remove,
        // while reporting success — and choosing which go first would be this
        // command inventing a policy nobody asked for.
        return Err(Error::Usage(format!(
            "{name} and the {} notes linking to it are {} in all, and a redaction \
             may name at most {}",
            linking.len(),
            targets.len(),
            capture_core::body::REDACT_TARGET_CAP
        )));
    }

    if !force {
        // `--force` suppresses this prompt and nothing else. The cap, the
        // window and the snapshot all remain — a flag that turned off the
        // safety rails as well would be the flag everyone reaches for.
        say!("This will permanently destroy the content of:");
        say!();
        say!("  {}", note.slug);
        if let Some(t) = &note.text {
            say!("  {}", first_line(t));
        }

        // Every name, because §1.1 makes this the one confirmation that must
        // not be trained away — and a flag that quietly widens what a familiar
        // prompt covers is exactly how it would be. Somebody who has typed the
        // name once has learned that the prompt means one note.
        if !linking.is_empty() {
            say!();
            say!(
                "...and the {} note(s) linking to it, which carry its name in \
                 their own text:",
                linking.len()
            );
            for b in &linking {
                say!("  {}", b.slug);
            }
        }

        // Named, because §1.1 makes this the one confirmation in the system
        // that must not be trained away — and it can only carry that weight if
        // it says what actually goes. A person who would hesitate over an audio
        // recording should not have to infer that it is included.
        //
        // It did not used to be included at all: `blobs::destroy` was written
        // to overwrite and unlink the bytes and was called by nothing, so
        // redaction destroyed the text and left the recording.
        let recordings = s.log.blobs_of(&tgt)?.len();
        match recordings {
            0 => {}
            1 => say!("  ...and the recording attached to it."),
            n => say!("  ...and the {n} recordings attached to it."),
        }
        say!();
        say!(
            "It cannot be undone after {} minutes.",
            capture_core::REDACT_WINDOW_MS / 60_000
        );
        say!("Type the note's name to confirm:");

        let mut typed = String::new();
        std::io::stdin().read_line(&mut typed)?;
        if typed.trim() != note.slug {
            // Not an error. Declining to destroy something is a legitimate
            // outcome and should not read like a failure.
            say!();
            say!("Nothing destroyed.");
            return Ok(());
        }
    }

    // Through the session's own redact, which records who the destruction has
    // to reach as well as writing the event. Authoring the event here directly
    // is what left every redaction reported complete the moment it was
    // written.
    let id = s.redact_all(&targets)?;

    say!();
    match targets.len() {
        1 => say!(
            "Queued. {} will be destroyed in {} minutes.",
            note.slug,
            capture_core::REDACT_WINDOW_MS / 60_000
        ),
        n => say!(
            "Queued. {} notes, including {}, will be destroyed in {} minutes.",
            n,
            note.slug,
            capture_core::REDACT_WINDOW_MS / 60_000
        ),
    }
    say!("  capture redact --cancel {}", hex8(&id));
    Ok(())
}

/// Stop every redaction issued since a moment.
///
/// The recovery for a mistake made in bulk — a script, or a selection that was
/// wider than intended. Separate from cancelling one because the two are
/// different admissions: one is "not that note", this is "not any of that".
pub fn redact_cancel_since(paths: StorePaths, since: u64) -> Result<()> {
    // The store's, rather than a second copy of the same filter. The window
    // arithmetic — comparing against when the redaction was *issued*, which is
    // the deadline less the window, because that is the moment a person is
    // thinking of — was written out twice and is the kind of thing that drifts
    // in one place and not the other.
    let n = capture_store::mutate::cancel_redactions_since(&paths, since)?;

    if n == 0 {
        say!("Nothing queued since then.");
        return Ok(());
    }
    say!("{n} redaction(s) cancelled. The content is readable again.");
    Ok(())
}

/// Stop a queued redaction.
pub fn redact_cancel(paths: StorePaths, id_prefix: &str) -> Result<()> {
    let mut s = crate::read::session(paths)?;

    let queued = s.state.redact_queue()?;
    let found = queued
        .iter()
        .find(|(redact, _, _, cancelled)| !cancelled && hex8(redact).starts_with(id_prefix));

    let Some((redact, tgt, _, _)) = found.copied() else {
        // The window may simply have elapsed. Saying so distinguishes "no such
        // redaction" from "you were too late", which are different situations
        // and want different reactions.
        return Err(Error::NotFound(format!(
            "no redaction waiting with id {id_prefix} — it may already have been carried out"
        )));
    };

    s.append(Kind::REDACT_CANCEL, Some(tgt), None, |nonce| {
        encode(&RedactCancelBody { nonce, redact })
    })?;
    say!("Cancelled. Nothing was destroyed.");
    Ok(())
}

fn hex8(id: &EventId) -> String {
    id.0[..4].iter().map(|b| format!("{b:02x}")).collect()
}

fn first_line(text: &str) -> String {
    let line = text.lines().next().unwrap_or("");
    let mut out: String = line.chars().take(70).collect();
    if line.chars().count() > 70 {
        out.push('…');
    }
    out
}

fn encode<T: minicbor::Encode<()>>(v: &T) -> Vec<u8> {
    let mut b = Vec::new();
    minicbor::encode(v, &mut b).expect("encoding into Vec is infallible");
    b
}

/// Change a recurring series from one occurrence onward.
///
/// # Why this is a split and not an edit
///
/// Editing the rule in place would change what the schedule *was* last March.
/// For an ordinary calendar that is a wrong entry; here it is worse, because a
/// log that quietly revises its own history is an unreliable narrator, and
/// compensating for unreliable memory is the whole point of the system. The
/// old rule is bounded just before the chosen occurrence and a new series
/// starts there, so the past stays exactly as it happened (§3.5).
///
/// The boundary is given as a date rather than an index. "The fourth one" is a
/// thing nobody knows about their own calendar, and it stops being true the
/// moment an occurrence is cancelled.
pub fn split_series(paths: StorePaths, name: &str, from: &str, rule_text: &str) -> Result<()> {
    let now = crate::read::now_ms();
    let done = capture_store::promote::split_at(&paths, name, from, rule_text, now, &zone_here())?;

    say!("Split {name} at {}.", day_of(done.boundary));
    say!(
        "  {} occurrence(s) before it keep the old rule",
        done.before
    );
    say!("  from it onward: {rule_text:?}");
    say!();
    say!("The past is unchanged: the old rule still says what it said, and");
    say!("what follows is a new note carrying the same text. Two entries with");
    say!("one name in the timeline is the split, not a duplicate.");
    Ok(())
}

/// This machine's zone, by name.
fn zone_here() -> String {
    capture_store::notifier::local_zone()
}

fn day_of(ms: i64) -> String {
    let tz = jiff::tz::TimeZone::system();
    jiff::Timestamp::from_millisecond(ms)
        .map(|t| t.to_zoned(tz).strftime("%Y-%m-%d").to_string())
        .unwrap_or_else(|_| "an unknown day".into())
}

/// Insert the character a capture was missing to be a list.
///
/// Says what it did, because it changed what somebody wrote. Accepting a type
/// leaves the words alone and this does not, and a surface that made the two
/// look alike would be teaching something false.
pub fn fix_list(paths: StorePaths, name: &str) -> Result<()> {
    let fixed = capture_store::mutate::accept_near_miss(&paths, name)?;
    say!("Now a list:");
    say!();
    say!("  {fixed}");
    Ok(())
}

/// Tick one item of a capture, by its number.
///
/// What to reach for when two items read the same: a number says which, and
/// words cannot. Counted from 1 on the way in, because the list prints itself
/// that way and somebody reading rows off a screen counts from one.
pub fn tick_at(paths: StorePaths, name: &str, ord: u32, done: bool) -> Result<()> {
    use capture_store::promote::Ticked;

    match capture_store::promote::tick_at(&paths, name, ord, done)? {
        Ticked::Done if done => say!("Done."),
        Ticked::Done => say!("Open again."),
        Ticked::Already if done => say!("Already done."),
        Ticked::Already => say!("Not ticked."),
        // An error, not a message. Asking to tick item 9 of a three-item list
        // is a request that could not be carried out, and saying so on stdout
        // while exiting 0 tells a script the tick landed. `tick-at <note> 0`
        // already exits non-zero, so the two bad item numbers disagreed about
        // whether they were failures.
        Ticked::NoSuchItem => {
            return Err(Error::NotFound(format!(
                "{name} has no item {}; they are numbered in the order they \
                 were written, from 1",
                ord + 1
            )));
        }
        // Unreachable by construction: a number names exactly one item, which
        // is the whole reason this exists beside `tick`.
        Ticked::Ambiguous(_) => say!("That item could not be told apart."),
    }
    Ok(())
}

/// Tick an item off a list, or put it back.
///
/// Says which of the four things happened rather than reporting success for
/// all of them. "Already done" and "just done" are different facts, and a
/// refusal to guess between two identical items is not a failure — it is the
/// only honest answer when completion matches by text.
pub fn tick(paths: StorePaths, tag: &str, item: &str, done: bool) -> Result<()> {
    use capture_store::promote::Ticked;

    match capture_store::promote::tick(
        &paths,
        tag,
        item,
        done,
        crate::read::now_ms().max(0) as u64,
    )? {
        Ticked::Done if done => say!("Done."),
        Ticked::Done => say!("Open again."),
        Ticked::Already if done => say!("Already done."),
        Ticked::Already => say!("Not ticked."),
        Ticked::Ambiguous(n) => {
            say!("{n} items on #{tag} read \"{item}\".");
            say!();
            say!("Naming an item by its words cannot tell those apart. Say which");
            say!("by its number instead:  capture tick-at <note> <n>");
        }
        Ticked::NoSuchItem => {
            say!("Nothing on #{tag} reads \"{item}\".");
            say!();
            say!("See what is on it with:  capture list {tag}");
        }
    }
    Ok(())
}

/// Read whatever has not been read yet.
///
/// A command somebody should never need, and that is the point of it: where a
/// daemon runs, it has already happened, and where one does not, this is what
/// the staleness line on a read has to be able to name. §3.1 defers the parse
/// so the capture gesture never waits for it, and §8.8 keeps reads off the
/// writer — between them there is nothing a read can do about being behind
/// except say so and point here.
///
/// Reports the count rather than succeeding silently, because "it read nothing"
/// and "it could not look" are different answers and conflating them is the
/// bug `settle` exists to stop. A held lock is an error and says who holds it.
pub fn settle(paths: &StorePaths, json: bool) -> Result<()> {
    let n = capture_store::promote::settle(paths)?;

    if json {
        say!("{{\"promoted\":{n}}}");
        return Ok(());
    }

    match n {
        0 => say!("Nothing was waiting to be read."),
        1 => say!("Read 1 capture."),
        n => say!("Read {n} captures."),
    }
    Ok(())
}

/// Copies kept before a redaction, and bringing one back.
///
/// §1.2 keeps a snapshot of what a redaction is about to destroy, for thirty
/// days, *"so that a misfire is recoverable during the window in which you
/// would notice it"* — explicitly for the case somebody notices long after the
/// cancellation window has closed.
///
/// Everything around it was built. `take_snapshot` is called before anything is
/// destroyed, `expire_snapshots` runs on every session open, and `capture
/// status` reports that the copies exist. The read had no caller at all, so a
/// plaintext copy of content somebody declared must not exist was kept for a
/// month, advertised, and unusable. The cost was paid in full and the purpose
/// was unreachable.
pub fn recover(paths: StorePaths, id: Option<&str>, json: bool) -> Result<()> {
    let now = now_ms().max(0) as u64;
    let ops = capture_store::ops::Ops::open_read(&paths.ops)?;
    let kept = ops.snapshots(now)?;

    let Some(wanted) = id else {
        return list_recoverable(&kept, now, json);
    };

    let matching: Vec<_> = kept
        .iter()
        .filter(|s| capture_store::mutate::short(&s.redact).starts_with(wanted))
        .collect();

    let found = match matching.as_slice() {
        [one] => *one,
        [] => {
            // An id that names nothing and an id whose copy has expired are
            // different situations, and only one of them is a typo.
            return Err(Error::NotFound(format!(
                "no copy is being kept for {wanted} — it may have expired, or the \
                 redaction may have been cancelled rather than carried out"
            )));
        }
        many => {
            return Err(Error::Usage(format!(
                "{} copies start with {wanted}; give more of it",
                many.len()
            )));
        }
    };

    // A new capture, never a resurrection. The body is gone from the log and
    // the redaction is *retained* so that it destroys matching events arriving
    // later — including one carrying this content from a peer. Writing the old
    // event back would be fought by the system's own machinery, and saying
    // "restored" would claim the redaction was undone, which is the more
    // dangerous of the two available lies: somebody would stop looking for the
    // other copies.
    let mut s = crate::read::session(paths.clone())?;
    let body = capture_store::session::capture_body(&found.text)?;
    let written = s.capture(&body)?;

    if json {
        say!("{{\"slug\":{}}}", quote(&written.slug));
        return Ok(());
    }

    say!("Written as a new note:  {}", written.slug);
    say!();
    say!("The redaction still stands. The original note is still destroyed, and");
    say!("this is a fresh capture carrying the text the copy held — a different");
    say!("note, with its own name.");
    Ok(())
}

fn list_recoverable(kept: &[capture_store::ops::Snapshot], now: u64, json: bool) -> Result<()> {
    if json {
        let items: Vec<String> = kept
            .iter()
            .map(|s| {
                // `taken_at` beside the retention: a consumer can do nothing
                // with a rendered date and everything with a number, and the
                // field has been carried this far and dropped at the surface.
                format!(
                    "{{\"id\":{},\"taken_at\":{},\"expires_in_ms\":{}}}",
                    quote(&capture_store::mutate::short(&s.redact)),
                    s.taken_at,
                    s.expires_at.saturating_sub(now)
                )
            })
            .collect();
        say!("[{}]", items.join(","));
        return Ok(());
    }

    if kept.is_empty() {
        say!("Nothing is being kept.");
        say!();
        say!("A copy is taken before a redaction destroys anything, and expires on");
        say!("its own. None is inside its retention now.");
        return Ok(());
    }

    say!("Copies of content that was destroyed, kept so a misfire is recoverable.");
    say!();
    let zone = local_zone();
    for s in kept {
        say!("{}", recoverable_line(s, now, zone));
    }
    say!();
    say!("Bring one back with:  capture recover <id>");
    say!("It is written as a new note; the redaction itself still stands.");
    Ok(())
}

/// Coarse enough to decide with, for a retention measured in days.
/// One line of the recoverable listing.
///
/// Pure, and lifted out of the loop, because it is the only way this can be
/// tested at all. Producing a snapshot needs a redaction that has actually been
/// carried out; the cancellation window is thirty minutes of real time (§1.1),
/// nothing drains the queue early, and the command line has no clock to inject.
/// So a test at that level would have to wait half an hour — which is why
/// `capture recover` is invoked by no test in the repo, and how a field could be
/// dropped here while the store test below it asserted the field was present.
///
/// The date rather than the instant: it is what somebody remembers — "the thing
/// I destroyed on Tuesday" — and a millisecond timestamp in a listing is noise
/// that makes the first line harder to find.
///
/// The first line of the text only. Enough to recognise a note, rather than a
/// second place the whole of a destroyed one is printed.
fn recoverable_line(
    s: &capture_store::ops::Snapshot,
    now: u64,
    zone: capture_time::Zone,
) -> String {
    format!(
        "  {}  taken {}  expires in {:<10}  {}",
        capture_store::mutate::short(&s.redact),
        capture_time::civil_date(s.taken_at as i64, zone),
        ms(s.expires_at.saturating_sub(now)),
        first_line(&s.text)
    )
}

fn ms(v: u64) -> String {
    let hours = v / (60 * 60 * 1000);
    match hours / 24 {
        0 => format!("{hours}h"),
        d => format!("{d}d"),
    }
}

/// Let a destruction the ceiling held go ahead.
///
/// §1.1 holds what is over the ceiling *pending explicit confirmation*, and
/// this is the confirming. It is deliberately a second command: the ceiling's
/// whole shape is that it costs a deliberate act to release and nothing to
/// ignore, so a person clearing out a year of notes types one more line and a
/// runaway loop stops and stays stopped.
///
/// Local, and never an event. A release says this device is satisfied, which is
/// not a fact about the archive — and a compromised device able to emit
/// releases would release its own runaway, which is handing the attacker the
/// key to the lock.
pub fn release_redaction(paths: StorePaths, id_prefix: &str) -> Result<()> {
    let ops = capture_store::ops::Ops::open(&paths.ops)?;
    let held = ops.held_redactions()?;

    let mut matching: Vec<_> = held
        .iter()
        .map(|(r, _)| *r)
        .filter(|r| capture_store::mutate::short(r).starts_with(id_prefix))
        .collect();
    matching.sort_by_key(|r| r.0);
    matching.dedup();

    let redact = match matching.as_slice() {
        [one] => *one,
        [] => {
            return Err(Error::NotFound(format!(
                "nothing is being held under {id_prefix} — capture status lists what is"
            )));
        }
        many => {
            return Err(Error::Usage(format!(
                "{} held redactions start with {id_prefix}; give more of it",
                many.len()
            )));
        }
    };

    let n = held.iter().filter(|(r, _)| *r == redact).count();
    ops.release_redaction(&redact, now_ms().max(0) as u64)?;

    say!("Released. {n} destruction(s) will be carried out on the next drain.");
    say!();
    say!("Nothing has been destroyed yet. It happens when the archive next");
    say!("settles — or immediately, with:  capture settle");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The listing says when a copy was taken.
    ///
    /// The first unit test in this file, and it exists because nothing else can
    /// reach this line: a snapshot needs a redaction that has actually been
    /// carried out, the cancellation window is thirty minutes of real time, and
    /// the command line has no clock to inject. `capture recover` is invoked by
    /// no test at all, which is how `taken_at` came to be stored, selected,
    /// ordered by, carried to the surface and dropped there.
    #[test]
    fn a_recoverable_copy_is_listed_with_when_it_was_taken() {
        let s = capture_store::ops::Snapshot {
            redact: capture_core::EventId([7; 32]),
            tgt: capture_core::EventId([9; 32]),
            // 2026-09-20, 15:00 UTC.
            taken_at: 1_789_916_400_000,
            expires_at: 1_792_508_400_000,
            text: "the thing I asked to be destroyed\nand a second line".into(),
        };
        let utc = capture_time::Zone::new("UTC").expect("UTC is always present");
        let line = recoverable_line(&s, 1_789_916_400_000, utc);

        assert!(
            line.contains("2026-09-20"),
            "the listing does not say when the copy was taken: {line}"
        );
        assert!(
            line.contains("the thing I asked to be destroyed"),
            "the listing does not say what it begins with: {line}"
        );
        // The first line only: the purpose is recognising a copy, not reading
        // destroyed content back out of a listing.
        assert!(
            !line.contains("and a second line"),
            "the listing printed more than the first line: {line}"
        );
    }
}
