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

use brain_core::{
    EventId,
    body::{
        AliasBody, EditBody, MuteBody, MuteScope, PinBody, RedactBody, RedactCancelBody,
        RedactScope, ResolveBody, TagBody, UntagBody,
    },
    event::Kind,
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
    s.append(Kind::EDIT, Some(tgt), Some(tgt), |nonce| {
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

/// Assign or clear a chosen name.
///
/// Refused here as well as in the fold. The fold's refusal is what makes two
/// partitioned devices agree; this one is what lets a person be told why.
/// Preferences this build understands, with the value used when none is chosen.
///
/// Compiled in, never written to the log on first run. Materialising defaults as
/// recorded decisions would freeze today's judgement permanently and destroy the
/// distinction between a value you picked and one you inherited (§8.9).
///
/// Here rather than in the command line because [`set_setting`] validates
/// against it, and that function is what every surface calls.
pub const KNOWN_SETTINGS: &[(&str, &str, &str)] = &[(
    "timeline.limit",
    "50",
    "how many notes `brain timeline` shows",
)];

/// The compiled-in value for a setting, if this build knows the key.
pub fn default_for(key: &str) -> Option<&'static str> {
    KNOWN_SETTINGS
        .iter()
        .find(|(k, _, _)| *k == key)
        .map(|(_, v, _)| *v)
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
        encode(&brain_core::body::ParentBody { nonce, parent })
    })?;
    Ok(())
}

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

/// Suppress everything matching a rule.
pub fn mute_scope(paths: &StorePaths, scope: MuteScope, until: Option<u64>) -> Result<()> {
    let mut s = Session::open(paths.clone())?;

    // A note-scoped mute names the note it is about.
    //
    // The fold does not need it — muting is a rule read from the body, and
    // which notes it covers is decided when the question is asked. `brain log
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
        encode(&brain_core::body::UnflagBody { nonce })
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

    let id = s.append(Kind::REDACT, Some(tgt), None, |nonce| {
        encode(&RedactBody {
            nonce,
            scope: RedactScope::WholeNote,
            targets: vec![tgt],
        })
    })?;
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
            !cancelled && execute_after.saturating_sub(brain_core::REDACT_WINDOW_MS) >= since
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
pub fn add_intent(paths: &StorePaths, cue: brain_core::body::Cue, then: &str) -> Result<EventId> {
    let cue_type = match &cue {
        brain_core::body::Cue::Time(_) => "time",
        brain_core::body::Cue::Recurring(_) => "recurring",
        brain_core::body::Cue::Location { .. } => "location",
        brain_core::body::Cue::AppFocus(_) => "app_focus",
        brain_core::body::Cue::CalendarStart(_) => "calendar_start",
        brain_core::body::Cue::DeviceWake => "device_wake",
        brain_core::body::Cue::GitBranch(_) => "git_branch",
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
        encode(&brain_core::body::IntentBody {
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
    // caller. A setting is an event: an unknown key written once is in the log
    // for ever, understood by nothing, and a second surface calling this
    // without the check would have been able to put one there.
    if default_for(key).is_none() {
        let known: Vec<&str> = KNOWN_SETTINGS.iter().map(|(k, _, _)| *k).collect();
        return Err(Error::Usage(format!(
            "'{key}' is not a setting this build knows.\n\nKnown: {}",
            known.join(", ")
        )));
    }

    let mut s = Session::open(paths.clone())?;
    s.append(Kind::SETTING, None, None, |nonce| {
        encode(&brain_core::body::SettingBody {
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
