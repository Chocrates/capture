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
//!
//! Three is the one that matters most and fires least. It means the archive
//! disagrees with itself, which is never the user's mistake and always worth
//! stopping for.

#[macro_use]
mod out;

mod ops;
mod pair;

mod args;
mod backup;
mod input;
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

    let command = match args::parse(argv) {
        Ok(c) => c,
        Err(bad) => {
            eprintln!("{}", bad.message());
            return ProcExit::from(ExitCode::Usage as u8);
        }
    };

    match run(command) {
        Ok(()) => ProcExit::from(ExitCode::Success as u8),
        Err(e) => {
            eprintln!("capture: {e}");
            ProcExit::from(e.exit_code() as u8)
        }
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
        Command::Capture(source) => capture(&source),
        Command::Timeline { limit, json } => read::timeline(&paths()?, limit, json),
        Command::Show { name, json } => read::show(&paths()?, &name, json),
        Command::Agenda { day, json } => read::agenda(&paths()?, day.as_deref(), json),
        Command::Tasks { json } => read::tasks(&paths()?, json),
        Command::List { tag, json } => read::list(&paths()?, &tag, json),
        Command::Due { json } => read::due(&paths()?, json),
        Command::Tick { tag, item, done } => write::tick(paths()?, &tag, &item, done),
        Command::TickAt { name, ord, done } => write::tick_at(paths()?, &name, ord, done),
        Command::FixList { name } => write::fix_list(paths()?, &name),
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
        Command::Occurrence { name, occ, done } => write::occurrence(paths()?, &name, occ, done),
        Command::Search { query, json } => read::search(&paths()?, &query, json),
        Command::Links { name, json } => read::links(&paths()?, &name, json),
        Command::Edit { name, text } => write::edit(paths()?, &name, &text),
        Command::Tag { name, tag } => write::tag(paths()?, &name, &tag),
        Command::Untag { name, tag } => write::untag(paths()?, &name, &tag),
        Command::Alias { name, alias } => write::alias(paths()?, &name, alias.as_deref()),
        Command::Pin { name, pinned } => write::pin(paths()?, &name, pinned),
        Command::Mute { what } => {
            let what = match what {
                args::MuteTarget::Note(n) => write::Muted::Note(n),
                args::MuteTarget::Tag(t) => write::Muted::Tag(t),
                args::MuteTarget::Days(a, b) => write::Muted::Days(a, b),
            };
            write::mute(paths()?, &what, None)
        }
        Command::Resolve { name } => write::resolve(paths()?, &name),
        Command::Log { name, json } => ops::log(&paths()?, &name, json),
        Command::Quarantine { json } => ops::quarantine(&paths()?, json),
        Command::Rejected { json } => ops::rejected(&paths()?, json),
        Command::Rebuild => ops::rebuild(paths()?),
        Command::Verify { json } => verify::run(&paths()?, json),
        Command::ExportsPruned => ops::exports_pruned(paths()?),
        Command::AdmissionSeen { key } => ops::admission_seen(paths()?, &key),
        Command::Peers { json } => ops::peers(&paths()?, json),
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
        Command::Redact { name, force } => write::redact(paths()?, &name, force),
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
    StorePaths::resolve().ok_or_else(|| {
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
    let fresh = store::create(
        &paths,
        jiff::Timestamp::now().as_millisecond().max(0) as u64,
    )?;

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
