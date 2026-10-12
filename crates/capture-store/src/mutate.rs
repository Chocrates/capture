//! Changing a note, without deciding how to say so.
//!
//! # Why this is not in either interface
//!
//! Two surfaces change notes — the command line and the desktop window — and
//! the design says explicitly that the graphical one is first-party rather than
//! a lesser twin. Two implementations of "add a tag" would drift, and the drift
//! would be in the rules: which events get written, what an untag retracts,
//! whether an alias collision is refused. Those are properties of the archive,
//! not of a surface.
//!
//! So the operations live here and return facts. What to print, whether to
//! prompt, and how to phrase a refusal belong to whichever interface asked.
//!
//! # Nothing here removes anything
//!
//! Every function appends. An edit records a new version beside the old one; an
//! untag names the specific additions it retracts; unpinning is a pin carrying
//! `false`. The one exception is redaction, which only *queues* — the
//! destruction itself is the purge, and it happens later and elsewhere.

use capture_core::{
    EventId,
    body::{
        AliasBody, EditBody, EntityBindBody, EntityMergeBody, EntityRenameBody, MuteBody,
        MuteScope, OccBody, OccEditBody, OccMoveBody, PinBody, RedactCancelBody, ResolveBody,
        TagBody, UntagBody,
    },
    event::Kind,
    id::EntId,
};

use crate::{Error, Result, StorePaths, session::Session};

/// Resolve a name, or say it is not there.
pub(crate) fn find(s: &Session, name: &str) -> Result<EventId> {
    s.state
        .resolve(name)?
        .ok_or_else(|| Error::NotFound(name.to_string()))
}

/// Replace a note's text with a new version.
pub fn edit(paths: &StorePaths, name: &str, text: &str) -> Result<()> {
    if text.trim().is_empty() {
        return Err(Error::Usage(
            "an edit with no text would leave the note unreadable; redact it if that is what \
             you mean"
                .into(),
        ));
    }
    let mut s = Session::open(paths.clone())?;
    let tgt = find(&s, name)?;

    // The version being edited, not the note.
    //
    // `prev` is what this edit follows, and it was the note's root every
    // time — so `resolve` (which calls a version a tip when nobody names it
    // as `prev`) saw every edit after the first as a tip. Two sequential
    // edits on one device, a minute apart, came back as "2 independent
    // versions". A conflict signal that fires when nothing conflicts is one
    // somebody learns to dismiss, and then it is not a signal.
    //
    // Falls back to the root where the projection has no head yet, which is
    // the pre-fold case and where the old behaviour was correct anyway.
    let prev = s.state.head_of(&tgt)?.unwrap_or(tgt);

    s.append(Kind::EDIT, Some(tgt), Some(prev), |nonce| {
        encode(&EditBody {
            nonce,
            text: text.to_string(),
        })
    })?;
    Ok(())
}

/// Add a tag.
pub fn tag(paths: &StorePaths, name: &str, tag: &str) -> Result<()> {
    let tag = tag.trim_start_matches('#');
    if tag.is_empty() {
        return Err(Error::Usage("a tag needs a name".into()));
    }
    let mut s = Session::open(paths.clone())?;
    let tgt = find(&s, name)?;
    s.append(Kind::TAG, Some(tgt), None, |nonce| {
        encode(&TagBody {
            nonce,
            tag: tag.to_string(),
        })
    })?;
    Ok(())
}

/// Retract a tag.
///
/// Names the specific additions it retracts rather than the word. Tags are an
/// add-wins set: a tag added on a device this one has not heard from must
/// survive a removal that never saw it, and that is only expressible by naming
/// what was actually observed.
pub fn untag(paths: &StorePaths, name: &str, tag: &str) -> Result<()> {
    let tag = tag.trim_start_matches('#');
    let mut s = Session::open(paths.clone())?;
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
    Ok(())
}

/// Put a page under another, or back at the top.
///
/// The one thing in this system that files something by hand. §3.7's argument
/// against filing is not withdrawn — it is why nothing outside the wiki sidebar
/// reads a parent, so the worst a wrong move can do is misplace a page in one
/// list. It remains in the timeline, in search, on every entity page that names
/// it and behind every link pointing at it.
///
/// The obvious loop is refused here, where it can be explained. Longer ones
/// cannot be: two devices can each write half of a cycle without either being
/// wrong at the time, so the fold breaks those afterwards and identically
/// everywhere.
pub fn set_parent(paths: &StorePaths, name: &str, under: Option<&str>) -> Result<()> {
    let mut s = Session::open(paths.clone())?;
    let tgt = find(&s, name)?;

    let parent = match under {
        None => None,
        Some(p) => {
            let id = find(&s, p)?;
            if id == tgt {
                return Err(Error::Usage(format!("'{name}' cannot sit under itself")));
            }
            // Walking up from the proposed parent: if this note is already an
            // ancestor of it, the move would close a loop. Refused with the
            // reason, because this one is a mistake somebody can see — unlike
            // the concurrent case, which nobody can.
            let mut at = id;
            let mut hops = 0;
            while let Some(next) = s.state.note(&at)?.and_then(|n| n.parent) {
                if next == tgt {
                    return Err(Error::Usage(format!(
                        "'{p}' already sits under '{name}', so this would make a loop"
                    )));
                }
                at = next;
                hops += 1;
                if hops > 1000 {
                    break;
                }
            }
            Some(id)
        }
    };

    s.append(Kind::PARENT, Some(tgt), None, |nonce| {
        encode(&capture_core::body::ParentBody { nonce, parent })
    })?;
    Ok(())
}

/// Assign or clear a chosen name.
///
/// Refused here as well as in the fold. The fold's refusal is what makes two
/// partitioned devices agree; this one is what lets a person be told why.
pub fn set_alias(paths: &StorePaths, name: &str, alias: Option<&str>) -> Result<()> {
    let mut s = Session::open(paths.clone())?;
    let tgt = find(&s, name)?;

    if let Some(new) = alias {
        if new.trim().is_empty() {
            return Err(Error::Usage("an alias needs a name".into()));
        }
        if let Some(holder) = s.state.resolve(new)?
            && holder != tgt
        {
            let held_by = s
                .state
                .note(&holder)?
                .map(|n| n.slug)
                .unwrap_or_else(|| "another note".into());
            return Err(Error::Usage(format!(
                "'{new}' already names {held_by}. Names are shared between slugs and aliases, \
                 so one name always means one note — otherwise a link written today could point \
                 somewhere else tomorrow."
            )));
        }
    }

    s.append(Kind::ALIAS, Some(tgt), None, |nonce| {
        encode(&AliasBody {
            nonce,
            alias: alias.map(String::from),
        })
    })?;
    Ok(())
}

/// Pin a note, or unpin it.
pub fn pin(paths: &StorePaths, name: &str, pinned: bool) -> Result<()> {
    let mut s = Session::open(paths.clone())?;
    let tgt = find(&s, name)?;
    s.append(Kind::PIN, Some(tgt), None, |nonce| {
        encode(&PinBody { nonce, pinned })
    })?;
    Ok(())
}

/// Suppress one note.
pub fn mute_note(paths: &StorePaths, name: &str) -> Result<()> {
    let mut s = Session::open(paths.clone())?;
    let tgt = find(&s, name)?;
    s.append(Kind::MUTE, Some(tgt), None, |nonce| {
        encode(&MuteBody {
            nonce,
            scope: MuteScope::Note(tgt),
            until: None,
        })
    })?;
    Ok(())
}

/// Lift one note's mute.
///
/// A `MUTE` whose end is `now`, because the fold already applies mutes newest
/// first and reads an elapsed end as not muted — so lifting is an ordinary
/// event, nothing is removed, and no new kind is needed.
///
/// Here rather than in each surface. The command line held the only copy,
/// so a note muted from the phone or the desktop app could be muted there and
/// lifted nowhere but a terminal — §2.6 says "reversible from any device".
///
/// Refused where nothing is muted: a note carries its mute as a field, so
/// "this was not muted" is a true thing to say rather than a guess, and an
/// event that changes nothing is noise in a log kept for ever.
pub fn unmute_note(paths: &StorePaths, name: &str, now: u64) -> Result<()> {
    let mut s = Session::open_at(paths.clone(), now)?;
    let tgt = find(&s, name)?;
    let in_force = s.state.note(&tgt)?.is_some_and(|n| n.is_muted(now));
    if !in_force {
        return Err(Error::NotFound(format!("{name} is not muted")));
    }
    s.append(Kind::MUTE, Some(tgt), None, |nonce| {
        encode(&MuteBody {
            nonce,
            scope: MuteScope::Note(tgt),
            until: Some(now),
        })
    })?;
    Ok(())
}

/// Suppress everything matching a rule.
pub fn mute_scope(paths: &StorePaths, scope: MuteScope, until: Option<u64>) -> Result<()> {
    let mut s = Session::open(paths.clone())?;

    // A note-scoped mute names the note it is about.
    //
    // The fold does not need it — muting is a rule read from the body, and
    // which notes it covers is decided when the question is asked. `capture log
    // <name>` does need it: that view filters on `tgt`, so a mute written
    // without one works perfectly and is missing from the history of the note
    // it silenced. "Every event that touched a note" would be false, quietly,
    // and only for mutes written through this function rather than through
    // `mute_note`.
    let tgt = match &scope {
        MuteScope::Note(t) => Some(*t),
        _ => None,
    };

    s.append(Kind::MUTE, tgt, None, |nonce| {
        encode(&MuteBody {
            nonce,
            scope: scope.clone(),
            until,
        })
    })?;
    Ok(())
}

/// Give an entity a name somebody chose.
///
/// The entity is named by a spelling, resolved through any merge, and the event
/// carries the `ent_id` — never the word. A rename written against a spelling
/// would be undone by the next rename, since the word it named has changed.
///
/// Refused where no note has ever used that spelling. §3.7's claim is that this
/// axis populates without curation, and the other side of that is that there is
/// nothing to name before something names it — a rename that created an entity
/// would be the filing step the axis exists to avoid.
pub fn rename_entity(paths: &StorePaths, kind: &str, name: &str, display: &str) -> Result<()> {
    if display.trim().is_empty() {
        return Err(crate::Error::Usage(String::from(
            "a name has to be something; use the spelling as written to clear it",
        )));
    }
    let ent = resolve_entity(paths, kind, name)?;
    let mut s = Session::open(paths.clone())?;
    s.append(Kind::ENTITY_RENAME, None, None, |nonce| {
        encode(&EntityRenameBody {
            nonce,
            ent,
            display: display.to_string(),
        })
    })?;
    Ok(())
}

/// Say that two entities were one.
///
/// Both are named by spelling and both are resolved first, so merging something
/// that has already been merged joins the chains rather than creating a second
/// answer.
///
/// Refused where the two resolve to the same entity: that is either a spelling
/// typed twice or a merge already done, and writing it would be an event that
/// says nothing.
pub fn merge_entities(paths: &StorePaths, kind: &str, from: &str, into: &str) -> Result<()> {
    let from_ent = resolve_entity(paths, kind, from)?;
    let into_ent = resolve_entity(paths, kind, into)?;
    if from_ent == into_ent {
        return Err(crate::Error::Usage(format!(
            "'{from}' and '{into}' are already the same {kind}",
        )));
    }
    let mut s = Session::open(paths.clone())?;
    s.append(Kind::ENTITY_MERGE, None, None, |nonce| {
        encode(&EntityMergeBody {
            nonce,
            from: from_ent,
            into: into_ent,
        })
    })?;
    Ok(())
}

/// Bind the note that *is* an entity, or unbind it.
///
/// `None` unbinds. The note is named the way every other command names one, so
/// an alias works, and it is resolved here so that the event carries an id — a
/// bind naming a slug would break the moment somebody renamed the note.
pub fn bind_backing(paths: &StorePaths, kind: &str, name: &str, note: Option<&str>) -> Result<()> {
    let ent = resolve_entity(paths, kind, name)?;
    let mut s = Session::open(paths.clone())?;
    let note = match note {
        Some(n) => Some(find(&s, n)?),
        None => None,
    };
    s.append(Kind::ENTITY_BIND, None, None, |nonce| {
        encode(&EntityBindBody { nonce, ent, note })
    })?;
    Ok(())
}

/// Suppress every note naming an entity.
///
/// A thin wrapper over [`mute_scope`], here so that callers name an entity the
/// way they name one everywhere else rather than deriving an id themselves.
pub fn mute_entity(paths: &StorePaths, kind: &str, name: &str, until: Option<u64>) -> Result<()> {
    let ent = resolve_entity(paths, kind, name)?;
    mute_scope(paths, MuteScope::Entity(ent), until)
}

/// The entity a spelling reaches, refusing where nothing has named it.
///
/// A pre-flight check with a prose refusal, in the shape `set_alias` uses. The
/// fold would decline a rename of an entity it has never seen by simply having
/// nothing to show it against — which is correct and is invisible, so the
/// refusal happens here where somebody is waiting for an answer.
fn resolve_entity(paths: &StorePaths, kind: &str, name: &str) -> Result<EntId> {
    let store = crate::projection::current(paths)?;
    match store.entity_id(kind, name)? {
        Some(ent) => Ok(ent),
        None => Err(crate::Error::NotFound(format!(
            "no {kind} called '{name}' — an entity exists because a note names it, \
             so write one first",
        ))),
    }
}

/// Insert the character a capture was missing to be a list.
///
/// An ordinary [`edit`], authored because somebody asked for it. The system does
/// not write this on its own: captured text is kept verbatim (§3.1.3), and a
/// correction to one's own words is a thing a person does.
///
/// Different in kind from accepting a promotion, which annotates and leaves the
/// text alone. That is why the surfaces say what will be inserted rather than
/// offering a bare yes — teaching somebody that accepting a suggestion never
/// touches what they wrote would be teaching them something false.
///
/// Refused where there is nothing to fix, so that a surface offering it on the
/// wrong capture fails loudly rather than rewriting text for no reason.
pub fn accept_near_miss(paths: &StorePaths, name: &str) -> Result<String> {
    let text = {
        let store = crate::projection::current(paths)?;
        let tgt = store
            .resolve(name)?
            .ok_or_else(|| crate::Error::NotFound(name.to_string()))?;
        store
            .text_of(&tgt)?
            .ok_or_else(|| crate::Error::Usage(String::from("that capture has no readable text")))?
    };

    let Some(miss) = capture_parse::near_miss(&text) else {
        return Err(crate::Error::Usage(String::from(
            "that capture is not one character from being a list",
        )));
    };

    let fixed = match &miss {
        // `#groceries 7 apples, …` becomes `#groceries: 7 apples, …`. The colon
        // goes directly after the tag, which is the only place the list form
        // ever looks for one.
        capture_parse::NearMiss::NoColon { tag } => {
            let trimmed = text.trim_start();
            let at = text.len() - trimmed.len() + 1 + tag.len();
            let mut fixed = String::with_capacity(text.len() + 1);
            fixed.push_str(&text[..at]);
            fixed.push(':');
            fixed.push_str(&text[at..]);
            fixed
        }
        // Nothing follows the colon, so there is nothing to insert and nothing
        // to guess. Saying so beats writing an empty list.
        capture_parse::NearMiss::NothingAfterTheColon { .. } => {
            return Err(crate::Error::Usage(String::from(
                "that one needs items, not a character; write them after the colon",
            )));
        }
    };

    edit(paths, name, &fixed)?;
    Ok(fixed)
}

/// Which half of the clock an unclear hour meant.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Half {
    Am,
    Pm,
}

impl Half {
    fn word(self) -> &'static str {
        match self {
            Half::Am => "am",
            Half::Pm => "pm",
        }
    }
}

/// Answer "2am or 2pm?" for a capture that wrote a bare hour.
///
/// An ordinary edit, like accepting a near miss and for the same reason: the
/// person's words are what the promotion is read from, so the answer goes into
/// the words — `at 2` becomes `at 2pm` — and the capture re-promotes with a
/// time. Writing a promotion beside unchanged text would leave the text still
/// saying something the calendar no longer does.
///
/// Inserted directly after the number and nowhere else, so nothing else the
/// person wrote moves. Refused where there is no unclear hour, so a surface
/// offering it on the wrong capture fails loudly instead of rewriting text.
pub fn settle_hour(paths: &StorePaths, name: &str, half: Half) -> Result<String> {
    let text = {
        let store = crate::projection::current(paths)?;
        let tgt = store
            .resolve(name)?
            .ok_or_else(|| crate::Error::NotFound(name.to_string()))?;
        store
            .text_of(&tgt)?
            .ok_or_else(|| crate::Error::Usage(String::from("that capture has no readable text")))?
    };

    let Some(unclear) = capture_parse::unclear_hour(&text) else {
        return Err(crate::Error::Usage(String::from(
            "that capture has no hour without am or pm",
        )));
    };

    let mut settled = String::with_capacity(text.len() + 2);
    settled.push_str(&text[..unclear.at]);
    settled.push_str(half.word());
    settled.push_str(&text[unclear.at..]);

    edit(paths, name, &settled)?;
    // Read again now rather than at the next sweep. The point of answering is
    // the time it gives, and a surface that showed "every Mon" straight after
    // the answer "pm" would read as the answer not having taken.
    crate::promote::settle(paths)?;
    Ok(settled)
}

/// Dismiss a note's divergence.
///
/// Dismissing is not choosing. Both versions stay in the log and in the version
/// list; what stops is the note being *reported* as diverged.
pub fn resolve(paths: &StorePaths, name: &str) -> Result<usize> {
    let mut s = Session::open(paths.clone())?;
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
    Ok(n)
}

/// Clear a note's credential flag.
pub fn unflag(paths: &StorePaths, name: &str) -> Result<()> {
    let mut s = Session::open(paths.clone())?;
    let tgt = find(&s, name)?;
    let note = s
        .state
        .note(&tgt)?
        .ok_or_else(|| Error::NotFound(name.to_string()))?;
    if !note.credential {
        return Err(Error::Usage(format!("{name} is not flagged")));
    }
    s.append(Kind::UNFLAG, Some(tgt), None, |nonce| {
        encode(&capture_core::body::UnflagBody { nonce })
    })?;
    Ok(())
}

/// Queue a redaction, returning the id that cancels it.
///
/// Only queues. The window, the cap and the snapshot are properties of the
/// operation and apply however it was reached — a confirmation is the
/// interface's job, and skipping one never skips these.
pub fn redact(paths: &StorePaths, name: &str) -> Result<String> {
    let mut s = Session::open(paths.clone())?;
    let tgt = find(&s, name)?;
    let note = s
        .state
        .note(&tgt)?
        .ok_or_else(|| Error::NotFound(name.to_string()))?;
    if note.is_redacted() {
        return Err(Error::Usage(format!("{name} has already been destroyed")));
    }

    let id = s.redact(tgt)?;
    Ok(short(&id))
}

/// Stop a queued redaction, named by the prefix `redact` returned.
pub fn cancel_redaction(paths: &StorePaths, id_prefix: &str) -> Result<()> {
    let mut s = Session::open(paths.clone())?;

    let found = s
        .state
        .redact_queue()?
        .into_iter()
        .find(|(redact, _, _, cancelled)| !cancelled && short(redact).starts_with(id_prefix));

    let Some((redact, tgt, _, _)) = found else {
        // The window may simply have elapsed, which is a different situation
        // from a mistyped id and wants a different reaction.
        return Err(Error::NotFound(format!(
            "no redaction waiting with id {id_prefix} — it may already have been carried out"
        )));
    };

    s.append(Kind::REDACT_CANCEL, Some(tgt), None, |nonce| {
        encode(&RedactCancelBody { nonce, redact })
    })?;
    Ok(())
}

/// Stop every redaction issued since a moment.
pub fn cancel_redactions_since(paths: &StorePaths, since: u64) -> Result<usize> {
    let mut s = Session::open(paths.clone())?;

    let doomed: Vec<(EventId, EventId)> = s
        .state
        .redact_queue()?
        .into_iter()
        .filter(|(_, _, execute_after, cancelled)| {
            // Against when the redaction was *issued* — the deadline less the
            // window — because that is the moment a person is thinking of.
            !cancelled && execute_after.saturating_sub(capture_core::REDACT_WINDOW_MS) >= since
        })
        .map(|(redact, tgt, _, _)| (redact, tgt))
        .collect();

    let n = doomed.len();
    for (redact, tgt) in doomed {
        s.append(Kind::REDACT_CANCEL, Some(tgt), None, |nonce| {
            encode(&RedactCancelBody { nonce, redact })
        })?;
    }
    Ok(n)
}

/// Make an if-then plan.
///
/// Refuses a cue this device cannot observe. Accepting one would produce an
/// intent that is registered, reported as working, and permanently silent —
/// the same failure shape as a chord that is bound and never fires, and the
/// one §2.4.1 is careful about.
pub fn add_intent(paths: &StorePaths, cue: capture_core::body::Cue, then: &str) -> Result<EventId> {
    let cue_type = match &cue {
        capture_core::body::Cue::Time(_) => "time",
        capture_core::body::Cue::Recurring(_) => "recurring",
        capture_core::body::Cue::Location { .. } => "location",
        capture_core::body::Cue::AppFocus(_) => "app_focus",
        capture_core::body::Cue::CalendarStart(_) => "calendar_start",
        capture_core::body::Cue::DeviceWake => "device_wake",
        capture_core::body::Cue::GitBranch(_) => "git_branch",
    };

    if !crate::notifier::observable(cue_type) {
        return Err(Error::Usage(format!(
            "nothing here can watch for '{cue_type}'.\n\
             \n  This device cannot observe it, and an intent it cannot see is one\n\
             that would look registered and never fire."
        )));
    }

    let mut s = Session::open(paths.clone())?;
    s.append(Kind::INTENT, None, None, |nonce| {
        encode(&capture_core::body::IntentBody {
            nonce,
            cue: cue.clone(),
            then: then.to_string(),
        })
    })
}

/// Abandon one.
///
/// Retracted rather than removed: a plan somebody gave up on is a fact about
/// what they tried, and §2.4 rates being easy to walk away from as part of the
/// case for having intents at all.
pub fn retract_intent(paths: &StorePaths, id: &EventId) -> Result<()> {
    let mut s = Session::open(paths.clone())?;
    s.append(Kind::TOMBSTONE, Some(*id), None, |_nonce| Vec::new())?;
    Ok(())
}

/// Record a preference.
pub fn set_setting(paths: &StorePaths, key: &str, value: Option<&str>) -> Result<()> {
    // Checked here rather than in the command line that used to be the only
    // caller. A setting is an event: a key or a value this build cannot use,
    // written once, is in the log for ever — and a second surface calling this
    // without the check would have been able to put one there.
    crate::settings::validate(key, value)?;

    let mut s = Session::open(paths.clone())?;
    s.append(Kind::SETTING, None, None, |nonce| {
        encode(&capture_core::body::SettingBody {
            nonce,
            key: key.to_string(),
            value: value.map(String::from),
        })
    })?;
    Ok(())
}

/// The prefix a person types to name a redaction.
pub fn short(id: &EventId) -> String {
    id.0[..4].iter().map(|b| format!("{b:02x}")).collect()
}

fn encode<T: minicbor::Encode<()>>(v: &T) -> Vec<u8> {
    let mut b = Vec::new();
    minicbor::encode(v, &mut b).expect("encoding into Vec is infallible");
    b
}

// ------------------------------------------------- one occurrence ----------
//
// Three of the five exception kinds were numbered, encoded, folded, rendered
// and tested in m2, and two of them were wired to a surface. `occ_done` and
// `occ_undone` are reachable as `capture done <series> <occ>`; `occ_cancel`,
// `occ_move` and `occ_edit` were reachable only from a hand-written append
// inside a test. The browse spec meanwhile describes a cancelled occurrence as
// "hidden but not gone", which is a state nothing could produce.
//
// These are in the store rather than in the command line — where `occurrence`
// still lives — because §8.4 wants every mutation reachable from the window
// too, and a second implementation there is how two surfaces come to disagree.

/// Skip one occurrence of a series, leaving the rule alone.
///
/// The gesture for "not this Thursday". Editing the rule would change every
/// future occurrence, and leaving the entry standing shows a meeting that is
/// not happening — so without this both available answers are wrong.
///
/// Named by its original start, which is what an occurrence *is* (§3.5): the
/// instant the rule would have produced, before any move. A moved occurrence
/// keeps that name, so skipping one somebody rescheduled still finds it.
///
/// Skipping what is already skipped is not an error. It is the state that was
/// asked for, and a refusal that depends on current state makes a gesture
/// somebody has to check before using.
pub fn skip_occurrence(paths: &StorePaths, name: &str, occ: i64) -> Result<()> {
    let mut s = Session::open(paths.clone())?;
    let tgt = series(&s, name, "skip")?;
    s.append(Kind::OCC_CANCEL, Some(tgt), None, |nonce| {
        encode(&OccBody { nonce, occ })
    })?;
    Ok(())
}

/// Put a skipped occurrence back.
///
/// Written as a *move* to the occurrence's own original start, because that is
/// how the fold resolves it: `apply_occ` sets `cancelled` and nothing clears
/// it, and m2 implemented a move beating a cancel on the same occurrence
/// deliberately — "a resurrected occurrence is visible and one gesture away; a
/// wrongly cancelled one is invisible and silently missed".
///
/// So there is no un-cancel event, and adding one would make that conflict rule
/// ambiguous rather than simpler. Moving something to where it already was is a
/// no-op about time and a statement about cancellation.
pub fn restore_occurrence(paths: &StorePaths, name: &str, occ: i64) -> Result<()> {
    let mut s = Session::open(paths.clone())?;
    let tgt = series(&s, name, "restore")?;
    s.append(Kind::OCC_MOVE, Some(tgt), None, |nonce| {
        encode(&OccMoveBody {
            nonce,
            occ,
            // Back to its own name, which is its original start by definition.
            start_ms: occ,
            // Whatever the rule says. An end invented here would put a length
            // on the calendar nobody chose, which is the same refusal the
            // promoter makes about §3.3's "30m default".
            end_ms: None,
        })
    })?;
    Ok(())
}

/// Move one occurrence of a series to a different time.
///
/// §3.5's first edit scope, "this occurrence": the rule is untouched and every
/// other occurrence keeps its time. The other two scopes are deliberately
/// different mechanisms — editing the rule moves past occurrences too, and
/// "this and future" is a split — and §3.5 names collapsing them as the thing
/// implementations get wrong.
///
/// The occurrence keeps its name. An occurrence is named by the start the rule
/// *would* have produced, so moving it says when it happens and not which one
/// it is — which is what lets a completion written before the move still apply
/// afterwards.
///
/// `end_ms` is passed through rather than invented. A length chosen here would
/// put a duration on the calendar nobody asked for, the same refusal
/// `restore_occurrence` makes and the same one the promoter makes about §3.3's
/// "30m default".
///
/// Distinct from [`restore_occurrence`], which writes the same kind to say
/// something else: that a cancellation is withdrawn. Expressing one as the
/// other would make a call site read "move this to where it already is".
pub fn move_occurrence(
    paths: &StorePaths,
    name: &str,
    occ: i64,
    start_ms: i64,
    end_ms: Option<i64>,
) -> Result<()> {
    let mut s = Session::open(paths.clone())?;
    let tgt = series(&s, name, "move")?;
    s.append(Kind::OCC_MOVE, Some(tgt), None, |nonce| {
        encode(&OccMoveBody {
            nonce,
            occ,
            start_ms,
            end_ms,
        })
    })?;
    Ok(())
}

/// Give one occurrence its own title, location or notes.
///
/// The first writer of `OCC_EDIT` anywhere. The kind has had a body type, a
/// fold arm and three projected columns since M2, and nothing in `mutate`, the
/// command line, the bindings, the window or the phone could author one — so
/// the columns were always empty and §3.5's "this occurrence only: different
/// title, location, notes" was a capability no person could reach.
///
/// `None` leaves that field to the series. Passing all three as `None` writes
/// an exception that overrides nothing, which the caller is expected to refuse
/// rather than record.
pub fn amend_occurrence(
    paths: &StorePaths,
    name: &str,
    occ: i64,
    title: Option<String>,
    location: Option<String>,
    notes: Option<String>,
) -> Result<()> {
    let mut s = Session::open(paths.clone())?;
    let tgt = series(&s, name, "amend")?;
    s.append(Kind::OCC_EDIT, Some(tgt), None, |nonce| {
        encode(&OccEditBody {
            nonce,
            occ,
            title: title.clone(),
            location: location.clone(),
            notes: notes.clone(),
        })
    })?;
    Ok(())
}

/// The note a series exception is about, refusing anything that is not one.
///
/// A plain note has no occurrences, and an exception naming it would sit in the
/// log for ever recording a mistake that changes nothing when folded — the
/// system silently right and the person silently wrong.
fn series(s: &Session, name: &str, doing: &str) -> Result<EventId> {
    let tgt = find(s, name)?;
    if s.state.occurrences_of(&tgt)?.is_empty() {
        return Err(Error::Usage(format!(
            "'{name}' has no occurrences, so there is nothing to {doing}. \
             Occurrences come from a repeating capture — \"standup every weekday at 9\""
        )));
    }
    Ok(tgt)
}
