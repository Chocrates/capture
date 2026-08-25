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

use brain_core::{
    EventId,
    body::{
        AliasBody, EditBody, MuteBody, MuteScope, PinBody, RedactBody, RedactCancelBody,
        RedactScope, ResolveBody, TagBody, UnflagBody, UntagBody,
    },
    event::Kind,
};
use brain_store::{Error, Result, StorePaths};

use brain_store::session::Session;

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
    let mut s = Session::open(paths)?;
    let tgt = find(&s, name)?;
    let head = s.state.note(&tgt)?.map(|n| n.tgt);

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
    let mut s = Session::open(paths)?;
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
    let mut s = Session::open(paths)?;
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
    let mut s = Session::open(paths)?;
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
    let mut s = Session::open(paths)?;
    let tgt = find(&s, name)?;
    s.append(Kind::PIN, Some(tgt), None, |nonce| {
        encode(&PinBody { nonce, pinned })
    })?;
    say!("{name} {}", if pinned { "pinned" } else { "unpinned" });
    Ok(())
}

/// Suppress a note from surfacing, without destroying it.
///
/// Muting exists so that making something go away is *cheaper* than destroying
/// it. If suppression were harder to reach than redaction, the gradient would
/// point at the irreversible operation.
pub fn mute(paths: StorePaths, what: &Muted, until: Option<u64>) -> Result<()> {
    let mut s = Session::open(paths)?;

    let (scope, tgt, described) = match what {
        Muted::Note(name) => {
            let tgt = find(&s, name)?;
            (MuteScope::Note(tgt), Some(tgt), name.clone())
        }
        Muted::Tag(tag) => {
            let tag = tag.trim_start_matches('#').to_string();
            (MuteScope::Tag(tag.clone()), None, format!("#{tag}"))
        }
        Muted::Days(from, to) => (
            MuteScope::DateRange(from.clone(), to.clone()),
            None,
            format!("{from} to {to}"),
        ),
    };

    s.append(Kind::MUTE, tgt, None, |nonce| {
        encode(&MuteBody {
            nonce,
            scope: scope.clone(),
            until,
        })
    })?;

    // Says what muting does, because it is easy to read as deletion — and the
    // whole reason it exists is to be the cheap thing you reach for instead.
    match until {
        Some(t) => say!("{described} muted until {t}"),
        None => say!("{described} muted"),
    }
    say!("It stays in the timeline and stops appearing in searches.");
    Ok(())
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
}

/// Dismiss a note's divergence.
///
/// Dismissing is not choosing. Both versions stay in the log and in the version
/// list; what stops is the note being *reported* as diverged. Nothing here can
/// discard the version that was not picked.
pub fn resolve(paths: StorePaths, name: &str) -> Result<()> {
    let mut s = Session::open(paths)?;
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
    let mut s = Session::open(paths)?;
    let tgt = find(&s, name)?;
    let note = s
        .state
        .note(&tgt)?
        .ok_or_else(|| Error::NotFound(name.to_string()))?;
    if !note.credential {
        return Err(Error::Usage(format!("{name} is not flagged")));
    }
    s.append(Kind::UNFLAG, Some(tgt), None, |nonce| {
        encode(&UnflagBody { nonce })
    })?;
    say!("{name} will be included in the next export");
    Ok(())
}

/// Destroy a note's content, after a window in which it can be stopped.
///
/// The only irreversible operation here, and the only one that prompts. The
/// prompt names what will go, individually — a count is dismissible in a way a
/// list of names is not, and this is the one confirmation in the system that
/// must not be trained away by appearing routinely.
pub fn redact(paths: StorePaths, name: &str, force: bool) -> Result<()> {
    let mut s = Session::open(paths)?;
    let tgt = find(&s, name)?;
    let note = s
        .state
        .note(&tgt)?
        .ok_or_else(|| Error::NotFound(name.to_string()))?;

    if note.is_redacted() {
        return Err(Error::Usage(format!("{name} has already been destroyed")));
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
        say!();
        say!(
            "It cannot be undone after {} minutes.",
            brain_core::REDACT_WINDOW_MS / 60_000
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

    let id = s.append(Kind::REDACT, Some(tgt), None, |nonce| {
        encode(&RedactBody {
            nonce,
            scope: RedactScope::WholeNote,
            targets: vec![tgt],
        })
    })?;

    say!();
    say!(
        "Queued. {} will be destroyed in {} minutes.",
        note.slug,
        brain_core::REDACT_WINDOW_MS / 60_000
    );
    say!("  brain redact --cancel {}", hex8(&id));
    Ok(())
}

/// Stop every redaction issued since a moment.
///
/// The recovery for a mistake made in bulk — a script, or a selection that was
/// wider than intended. Separate from cancelling one because the two are
/// different admissions: one is "not that note", this is "not any of that".
pub fn redact_cancel_since(paths: StorePaths, since: u64) -> Result<()> {
    let mut s = Session::open(paths)?;

    let doomed: Vec<(EventId, EventId)> = s
        .state
        .redact_queue()?
        .into_iter()
        .filter(|(_, _, execute_after, cancelled)| {
            // Compared against when the redaction was *issued*, which is the
            // deadline less the window — the user is thinking about when they
            // did the thing, not when it lands.
            !cancelled && execute_after.saturating_sub(brain_core::REDACT_WINDOW_MS) >= since
        })
        .map(|(redact, tgt, _, _)| (redact, tgt))
        .collect();

    if doomed.is_empty() {
        say!("No redactions were issued after that point.");
        return Ok(());
    }

    let n = doomed.len();
    for (redact, tgt) in doomed {
        s.append(Kind::REDACT_CANCEL, Some(tgt), None, |nonce| {
            encode(&RedactCancelBody { nonce, redact })
        })?;
    }
    say!("Cancelled {n} redaction(s). Nothing was destroyed.");
    Ok(())
}

/// Stop a queued redaction.
pub fn redact_cancel(paths: StorePaths, id_prefix: &str) -> Result<()> {
    let mut s = Session::open(paths)?;

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
