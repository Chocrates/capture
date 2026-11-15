//! `capture` — capture first, organise later.
//!
//! # Exit codes are part of the interface
//!
//! Four of them, and they are distinct because the distinctions are the useful
//! part: a script cannot tell "you typed it wrong" from "that note is gone"
//! from "the archive is damaged" if all three come back as 1.
//!
//! | code | meaning |
//! |------|---------|
//! | 0    | it worked |
//! | 1    | the request was wrong |
//! | 2    | the thing asked for does not exist |
//! | 3    | stored data failed verification |
//! | 4    | the note moved on since the version named; nothing written |
//! | 5    | refused on purpose (an agent writing a credential); nothing written |
//!
//! Three is the one that matters most and fires least. It means the archive
//! disagrees with itself, which is never the user's mistake and always worth
//! stopping for.

#[macro_use]
mod out;

mod ops;
mod pair;

mod agent;

mod args;
mod backup;
mod doctor;
mod input;
mod json;
mod read;
mod shell;
mod verify;
mod write;

use std::process::ExitCode as ProcExit;

use capture_store::{Error, ExitCode, Result, StorePaths, store};

use capture_store::session::{self, Committed};

use crate::args::{Command, Source};

fn main() -> ProcExit {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "warn".into()),
        )
        .with_writer(std::io::stderr)
        .init();

    let argv: Vec<String> = std::env::args().skip(1).collect();
    let (argv, write_json) = split_write_json(argv);
    if write_json {
        out::write_json();
    }

    let command = match args::parse(argv) {
        Ok(c) => c,
        Err(bad) => {
            if write_json {
                eprintln!("{}", error_json("usage", &bad.message()));
            } else {
                eprintln!("{}", bad.message());
            }
            return ProcExit::from(ExitCode::Usage as u8);
        }
    };

    let touched = touched(&command);
    let outcome = acting()
        .map_or(Ok(()), |name| agent_may(&command, &name))
        .and_then(|()| run(command));
    let outcome = outcome.and_then(|()| {
        if write_json {
            wrote_json(touched.or_else(out::written).as_deref())
        } else {
            Ok(())
        }
    });
    match outcome {
        Ok(()) => ProcExit::from(ExitCode::Success as u8),
        Err(e) => {
            if write_json {
                eprintln!("{}", error_json(code_name(e.exit_code()), &e.to_string()));
            } else {
                eprintln!("capture: {e}");
            }
            ProcExit::from(e.exit_code() as u8)
        }
    }
}

/// The commands whose `--json` this layer answers, because they write.
///
/// Reads parse `--json` themselves and render their own view; a write's view is
/// the same for every write — what it touched — so it is answered once, here.
const WRITE_VERBS: &[&str] = &[
    "edit",
    "append",
    "tag",
    "untag",
    "alias",
    "parent",
    "pin",
    "unpin",
    "page",
    "unpage",
    "tick-at",
    "untick-at",
];

/// Take `--json` off a write command, saying whether it was there.
///
/// Before `--` only: after it, every word is the note's. For `+`, only a
/// leading `capture --json + ...`, because any word after `+` is content.
/// A leading `--json` before a read is put back after its verb.
fn split_write_json(mut argv: Vec<String>) -> (Vec<String>, bool) {
    let leading = argv.first().is_some_and(|a| a == "--json");
    if leading {
        argv.remove(0);
    }
    let Some(verb) = argv.first().cloned() else {
        return (argv, leading);
    };
    if verb == "+" {
        return (argv, leading);
    }
    if !WRITE_VERBS.contains(&verb.as_str()) {
        if leading {
            argv.insert(1, "--json".into());
        }
        return (argv, false);
    }
    let cut = argv.iter().position(|a| a == "--").unwrap_or(argv.len());
    let before = argv[..cut].len();
    let mut kept: Vec<String> = argv[..cut]
        .iter()
        .filter(|a| *a != "--json")
        .cloned()
        .collect();
    let found = leading || kept.len() != before;
    kept.extend(argv[cut..].iter().cloned());
    (kept, found)
}

/// The note a write names on its command line, where it names one.
fn touched(c: &Command) -> Option<String> {
    match c {
        Command::Edit { name, .. }
        | Command::Append { name, .. }
        | Command::Tag { name, .. }
        | Command::Untag { name, .. }
        | Command::Parent { name, .. }
        | Command::Pin { name, .. }
        | Command::MakePage { name }
        | Command::TickAt { name, .. } => Some(name.clone()),
        // Renamed: the note answers to the new name now.
        Command::Alias { name, alias } => Some(alias.clone().unwrap_or_else(|| name.clone())),
        _ => None,
    }
}

/// What a write answers under `--json`: the note as it now stands.
fn wrote_json(name: Option<&str>) -> Result<()> {
    use crate::json::obj;
    let Some(name) = name else {
        out::answer_line(&obj([("ok", true.into())]).render());
        return Ok(());
    };
    let state = read::open(&paths()?)?;
    let tgt = state
        .resolve(name)?
        .ok_or_else(|| Error::NotFound(name.to_string()))?;
    let row = state
        .note(&tgt)?
        .ok_or_else(|| Error::NotFound(name.to_string()))?;
    let version = state.head_of(&tgt)?.unwrap_or(tgt);
    let kind = state
        .promotion_of(&tgt)?
        .map_or_else(|| "note".to_string(), |p| p.kind);
    let shown = row.alias.clone().unwrap_or_else(|| row.slug.clone());
    out::answer_line(
        &obj([
            ("name", shown.into()),
            ("slug", row.slug.clone().into()),
            ("version", capture_core::id::hexed(&version.0).into()),
            ("kind", kind.into()),
            ("by", row.by.clone().into()),
            ("notice", out::noticed().into()),
        ])
        .render(),
    );
    Ok(())
}

fn error_json(code: &str, message: &str) -> String {
    use crate::json::obj;
    obj([("error", code.into()), ("message", message.into())]).render()
}

fn code_name(c: ExitCode) -> &'static str {
    match c {
        ExitCode::Success => "ok",
        ExitCode::Usage => "usage",
        ExitCode::NotFound => "not_found",
        ExitCode::Integrity => "integrity",
        ExitCode::Conflict => "conflict",
        ExitCode::Refused => "refused",
    }
}

/// The agent this invocation writes as, from `CAPTURE_AS`.
fn acting() -> Option<String> {
    std::env::var(capture_store::paths::AS_ENV)
        .ok()
        .filter(|v| !v.is_empty())
}

/// What an agent may run: reading, and writing notes. Nothing else.
///
/// An allowlist, so a command added later is refused as an agent until
/// somebody decides otherwise. Everything left out is the person's: destroying,
/// membership, settings, the calendar, corrections, review, plans that ring.
fn agent_may(c: &Command, name: &str) -> Result<()> {
    let ok = matches!(
        c,
        Command::Help
            | Command::Version
            | Command::Status
            | Command::Timeline { .. }
            | Command::Show { .. }
            | Command::Cal { .. }
            | Command::Agenda { .. }
            | Command::Tasks { .. }
            | Command::List { .. }
            | Command::Due { .. }
            | Command::Mentions { .. }
            | Command::People { .. }
            | Command::Projects { .. }
            | Command::Entity { .. }
            | Command::Intents { .. }
            | Command::Read { .. }
            | Command::Search { .. }
            | Command::Links { .. }
            | Command::Log { .. }
            | Command::AgentList { .. }
            | Command::AgentInstructions { .. }
            | Command::Capture(_)
            | Command::NewPage { .. }
            | Command::NewPageFor { .. }
            | Command::MakePage { .. }
            | Command::Edit { .. }
            | Command::Append { .. }
            | Command::Tag { .. }
            | Command::Untag { .. }
            | Command::Alias { .. }
            | Command::Parent { .. }
            | Command::Pin { .. }
            | Command::TickAt { .. }
    );
    if ok {
        Ok(())
    } else {
        Err(Error::Refused(format!(
            "{name} is an agent (CAPTURE_AS), and this command is the person's to run; \
             nothing was done"
        )))
    }
}

fn run(command: Command) -> Result<()> {
    match command {
        Command::Help => {
            emit!("{}", args::USAGE);
            Ok(())
        }
        Command::Version => {
            say!("capture {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        Command::Init => init(),
        Command::Status => ops::status(&paths()?),
        Command::Doctor { ask } => doctor::run(&paths()?, ask),
        Command::Capture(source) => capture(&source),
        Command::Timeline { limit, json } => read::timeline(&paths()?, limit, json),
        Command::Show { name, json } => read::show(&paths()?, &name, json),
        Command::Cal { span, json } => read::cal(&paths()?, &span, json),
        Command::Agenda { day, json } => read::agenda(&paths()?, day.as_deref(), json),
        Command::Tasks { json } => read::tasks(&paths()?, json),
        Command::Settle { json } => write::settle(&paths()?, json),
        Command::Held { json } => ops::held(&paths()?, json),
        Command::Recover { id, json } => write::recover(paths()?, id.as_deref(), json),
        Command::ReleaseRedaction { id } => write::release_redaction(paths()?, &id),
        Command::Skip { name, occ, skipped } => write::skip(&paths()?, &name, &occ, skipped),
        Command::Move { name, occ, to } => write::move_occurrence(&paths()?, &name, &occ, &to),
        Command::Amend {
            name,
            occ,
            title,
            location,
            notes,
        } => write::amend_occurrence(&paths()?, &name, &occ, title, location, notes),
        Command::List { tag, json } => read::list(&paths()?, &tag, json),
        Command::Due { json } => read::due(&paths()?, json),
        Command::Tick { tag, item, done } => write::tick(paths()?, &tag, &item, done),
        Command::TickAt { name, ord, done } => write::tick_at(paths()?, &name, ord, done),
        Command::FixList { name } => write::fix_list(paths()?, &name),
        Command::Hour { name, pm } => write::hour(paths()?, &name, pm),
        Command::Day { name, which } => write::day(paths()?, &name, which),
        Command::Reread { name } => write::reread(paths()?, &name),
        Command::NewPage {
            title,
            under,
            stdin,
        } => write::new_page(paths()?, &title, under.as_deref(), stdin),
        Command::NewPageFor { title, of } => write::new_page_for(paths()?, &title, &of),
        Command::MakePage { name } => write::make_page(paths()?, &name),
        Command::Mentions { name, json } => read::mentions(&paths()?, &name, json),
        Command::People { json } => read::entities(&paths()?, "person", json),
        Command::Projects { json } => read::entities(&paths()?, "project", json),
        Command::Entity { name, json } => read::entity(&paths()?, &name, json),
        Command::EntityRename { name, display } => write::entity_rename(paths()?, &name, &display),
        Command::EntityMerge { from, into } => write::entity_merge(paths()?, &from, &into),
        Command::EntityBind { name, note } => write::entity_bind(paths()?, &name, note.as_deref()),
        Command::Parent { name, under } => write::parent(paths()?, &name, under.as_deref()),
        Command::Forget { id } => write::forget(paths()?, &id),
        Command::Task { name, due } => write::task(paths()?, &name, due.as_deref()),
        Command::TaskDone { name, done } => write::task_done(paths()?, &name, done),
        Command::Review { json } => read::review(&paths()?, json),
        Command::Intent { cue, arg, then } => write::intent(paths()?, &cue, &arg, &then),
        Command::Intents { json } => read::intents(&paths()?, json),
        Command::During { name, json } => read::during(&paths()?, &name, json),
        Command::Read { text } => read::explain(&text),
        Command::CarriesForward { name } => write::carries_forward(paths()?, &name),
        Command::Reviewed { name, outcome } => write::reviewed(paths()?, &name, &outcome),
        Command::Occurrence { name, occ, done } => write::occurrence(paths()?, &name, &occ, done),
        Command::Search { query, json } => read::search(&paths()?, &query, json),
        Command::Links { name, json } => read::links(&paths()?, &name, json),
        Command::Edit { name, text, expect } => {
            write::edit(paths()?, &name, &text, expect.as_deref())
        }
        Command::Append { name, text } => write::append(paths()?, &name, &text),
        Command::AgentAdd { name } => agent::add(&paths()?, &name),
        Command::AgentList { json } => agent::list(&paths()?, json),
        Command::AgentInstructions { name } => agent::instructions(&name),
        Command::Tag { name, tag } => write::tag(paths()?, &name, &tag),
        Command::Untag { name, tag } => write::untag(paths()?, &name, &tag),
        Command::Alias { name, alias } => write::alias(paths()?, &name, alias.as_deref()),
        Command::Pin { name, pinned } => write::pin(paths()?, &name, pinned),
        Command::Mute { what, until } => {
            let until = match until {
                Some(day) => Some(write::mute_ends(&day)?),
                None => None,
            };
            write::mute(paths()?, &muted(what), until)
        }
        Command::Unmute { what } => write::unmute(paths()?, &muted(what)),
        Command::Resolve { name } => write::resolve(paths()?, &name),
        Command::Log { name, json } => ops::log(&paths()?, &name, json),
        Command::Quarantine { json } => ops::quarantine(&paths()?, json),
        Command::Rejected { json } => ops::rejected(&paths()?, json),
        Command::Rebuild => ops::rebuild(paths()?),
        Command::Verify { json } => verify::run(&paths()?, json),
        Command::ExportsPruned => ops::exports_pruned(paths()?),
        Command::AdmissionSeen { key } => ops::admission_seen(paths()?, &key),
        Command::Peers { json } => ops::peers(&paths()?, json),
        Command::PeersAdd { addr } => ops::peers_add(&paths()?, &addr),
        Command::Split { name, from, rule } => write::split_series(paths()?, &name, &from, &rule),
        Command::Retire { key, force } => pair::retire(paths()?, &key, force),
        Command::Cosign { blob } => pair::cosign(paths()?, &blob),
        Command::Readmit { cosign, by } => {
            pair::readmit(paths()?, cosign.as_deref(), by.as_deref())
        }
        Command::Pair { code, at } => match code {
            Some(c) => pair::accept(paths()?, &c, at.as_deref()),
            None => pair::offer(paths()?),
        },
        Command::Export { json } => backup::export(paths()?, json),
        Command::Restore {
            from,
            ignore_redactions_after,
        } => backup::restore(
            paths()?,
            std::path::Path::new(&from),
            ignore_redactions_after,
        ),
        Command::Unflag { name } => write::unflag(paths()?, &name),
        Command::Redact {
            name,
            force,
            with_links,
        } => write::redact(paths()?, &name, force, with_links),
        Command::RedactCancel { id } => write::redact_cancel(paths()?, &id),
        Command::RedactCancelSince { since } => write::redact_cancel_since(paths()?, since),
        Command::Set { key, value } => ops::set(paths()?, &key, value.as_deref()),
        Command::ShellInit(name) => {
            emit!("{}", shell::Shell::parse(&name)?.init());
            Ok(())
        }
    }
}

fn paths() -> Result<StorePaths> {
    StorePaths::resolve()
        .map(|p| p.acting_as(acting()))
        .ok_or_else(|| {
            Error::Usage(
                "cannot work out where the store should live: no home directory, and \
             $CAPTURE_HOME is not set"
                    .into(),
            )
        })
}

/// Capture text, and say where it went.
///
/// The text is read *before* the store is opened. Reading first means a capture
/// typed at the prompt is not lost to a store that turns out to be locked or
/// missing — the message can quote the text back, and it is still on the
/// terminal to copy.
fn capture(source: &Source) -> Result<()> {
    let text = input::read(source)?;
    let paths = paths()?;

    match session::capture(&paths, &text)? {
        Committed::Stored(slug) => {
            out::wrote(&slug);
            say!("{slug}");
            Ok(())
        }
        Committed::Spooled => {
            // Said immediately, not saved for a daily summary: the condition
            // gets worse with every further capture rather than with the
            // calendar.
            say!("spooled — the store could not be written");
            say!();
            say!("Your capture is safe and will be committed as soon as the store");
            say!("accepts writes again. It will not appear in searches until then.");
            say!("  capture status");
            Ok(())
        }
    }
}

fn init() -> Result<()> {
    let paths = paths()?;
    let fresh = store::create(&paths, read::now_ms() as u64)?;

    // The roster root is authored by `create` but not stored by it, because
    // storing is this layer's job. Nothing else can be written until it is in
    // the log: it is what says which key this origin authors under.
    let mut log = capture_store::log::Log::open(&paths.events)?;
    log.append(&fresh.root)?;
    log.append(&fresh.roster_key)?;

    let ops = capture_store::ops::Ops::open(&paths.ops)?;
    let generation = ops.generation()?;
    capture_store::projection::Store::open(&capture_store::projection::generation_path(
        &paths.state,
        generation,
    ))?;

    let (wall, counter) = fresh.clock.persist();
    let mut identity = fresh.identity;
    identity.advance(&paths, fresh.roster_key.header.seq, wall, counter)?;

    say!("store created at {}", paths.root.display());
    say!("device {}", hex(&identity.dev().short()));
    say!();
    say!("capture something:  capture + your first note");
    Ok(())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// One translation from the argument enum to the write one.
///
/// Both exist on purpose — `args` holds what somebody typed and `write` holds
/// what the store is being asked for — but the mapping between them is one
/// decision, and it was about to be written twice once `unmute` needed it.
fn muted(what: args::MuteTarget) -> write::Muted {
    match what {
        args::MuteTarget::Note(n) => write::Muted::Note(n),
        args::MuteTarget::Tag(t) => write::Muted::Tag(t),
        args::MuteTarget::Days(a, b) => write::Muted::Days(a, b),
        args::MuteTarget::Entity(k, n) => write::Muted::Entity(k, n),
    }
}
