//! `brain` — capture first, organise later.
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

mod args;
mod input;
mod session;

use std::process::ExitCode as ProcExit;

use brain_store::{Error, ExitCode, Result, StorePaths, store};

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
            eprintln!("brain: {e}");
            ProcExit::from(e.exit_code() as u8)
        }
    }
}

fn run(command: Command) -> Result<()> {
    match command {
        Command::Help => {
            print!("{}", args::USAGE);
            Ok(())
        }
        Command::Version => {
            println!("brain {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        Command::Init => init(),
        Command::Status => status(),
        Command::Capture(source) => capture(&source),
    }
}

fn paths() -> Result<StorePaths> {
    StorePaths::resolve().ok_or_else(|| {
        Error::Usage(
            "cannot work out where the store should live: no home directory, and \
             $BRAIN_HOME is not set"
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
    let mut s = session::Session::open(paths()?)?;
    let c = s.capture(&text)?;
    println!("{}", c.slug);
    Ok(())
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
    let mut log = brain_store::log::Log::open(&paths.events)?;
    log.append(&fresh.root)?;

    let ops = brain_store::ops::Ops::open(&paths.ops)?;
    let generation = ops.generation()?;
    brain_store::projection::Store::open(&brain_store::projection::generation_path(
        &paths.state,
        generation,
    ))?;

    let (wall, counter) = fresh.clock.persist();
    let mut identity = fresh.identity;
    identity.advance(&paths, fresh.root.header.seq, wall, counter)?;

    println!("store created at {}", paths.root.display());
    println!("device {}", hex(&identity.dev().short()));
    println!();
    println!("capture something:  brain + your first note");
    Ok(())
}

fn status() -> Result<()> {
    let paths = paths()?;
    if !store::exists(&paths) {
        return Err(Error::StoreMissing(paths.root.clone()));
    }

    // Read-only throughout. Status is a diagnostic, and a diagnostic that needs
    // the write lock cannot answer the question "why will nothing write?".
    let log = brain_store::log::Log::open_read(&paths.events)?;
    let ops = brain_store::ops::Ops::open_read(&paths.ops)?;
    let generation = ops.generation()?;

    println!("store       {}", paths.root.display());
    println!("events      {}", log.count()?);
    println!("generation  {generation}");

    // Ask the kernel, not the file. The lock file outlives the process that
    // held it, so its contents alone cannot tell a live writer from one that
    // was killed — and reporting a dead process as the current writer is the
    // pid-file failure the advisory lock exists to avoid. Taking the lock and
    // immediately dropping it is the only honest test.
    let lock_path = paths.root.join("writer.lock");
    match brain_store::lock::Writer::acquire(&lock_path, "status")? {
        Some(_) => println!("writer      none"),
        None => {
            let who =
                brain_store::lock::holder(&lock_path).unwrap_or_else(|| "another process".into());
            println!("writer      {who}");
        }
    }

    // The condition that used to be a refusal. A device whose logical clock
    // runs ahead of its wall clock is stamping events into the future and
    // winning every last-writer-wins comparison until the wall clock catches
    // up. That is worth saying out loud — and never worth dropping a capture
    // over, which is why it is reported here rather than enforced at the point
    // of writing.
    let identity = brain_store::identity::Identity::load(&paths)?;
    let w = identity.watermark();
    let lead = brain_core::hlc::HlcState::restore(w.hlc_wall, w.hlc_counter)
        .lead_ms(jiff::Timestamp::now().as_millisecond().max(0) as u64);
    if lead > brain_core::hlc::LEAD_REPORT_MS {
        println!(
            "clock       {} ahead of this machine's wall clock",
            ms(lead)
        );
        println!("            some device's clock is wrong; ordering is distorted");
        println!("            until it is corrected. Captures still work.");
    }

    let rejected = ops.rejected_total()?;
    if rejected > 0 {
        // Reported separately and never folded into anything else: an event
        // that failed authentication is a security signal, and burying it in a
        // total alongside version skew is how it goes unnoticed.
        println!("rejected    {rejected}");
        for (reason, n) in ops.rejected_counts()? {
            println!("            {n} {reason}");
        }
    }
    Ok(())
}

/// A duration a person can read, from milliseconds.
fn ms(v: u64) -> String {
    let secs = v / 1000;
    match secs {
        0..=59 => format!("{secs}s"),
        60..=3599 => format!("{}m{}s", secs / 60, secs % 60),
        _ => format!("{}h{}m", secs / 3600, (secs % 3600) / 60),
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
