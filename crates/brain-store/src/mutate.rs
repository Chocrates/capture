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
    s.append(Kind::MUTE, None, None, |nonce| {
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

/// Record a preference.
pub fn set_setting(paths: &StorePaths, key: &str, value: Option<&str>) -> Result<()> {
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
