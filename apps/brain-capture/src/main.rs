//! `brain-capture` — the resident overlay.
//!
//! Owns the capture gesture on the desktop: chooses a mechanism the running
//! session actually supports, registers the chord with it, and (from task 8.4)
//! keeps a pre-warmed window ready.
//!
//! # Registering is a separate act from running
//!
//! `--register` writes the binding and exits. The overlay itself does not
//! register on every start, because the binding is a change to the user's
//! desktop configuration and a program that quietly rewrites that on each
//! launch is one that cannot be told to stop.

mod autostart;
mod due;
mod gnome;
mod overlay;
mod portal;
mod socket;
mod window;
mod x11;

use std::process::ExitCode as ProcExit;

use brain_store::{ExitCode, Result, StorePaths};

use brain_store::hotkey::{self, DEFAULT_CHORD, Mechanism};

fn main() -> ProcExit {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .with_writer(std::io::stderr)
        .init();

    let arg = std::env::args().nth(1).unwrap_or_default();
    let r = match arg.as_str() {
        "--run" => run(),
        "--show" => show(),
        "--register" => register(),
        "--autostart" => enable_autostart(),
        "--no-autostart" => disable_autostart(),
        "--unregister" => unregister(),
        "" | "--status" => report(),
        other => {
            eprintln!("brain-capture: unknown option {other}");
            eprintln!();
            eprintln!("  --run           hold the window ready (what autostart runs)");
            eprintln!("  --show          reveal it (what the chord runs)");
            eprintln!("  --register      bind the chord, then confirm it arrives");
            eprintln!("  --autostart     start at login");
            eprintln!("  --status        what is in force");
            return ProcExit::from(ExitCode::Usage as u8);
        }
    };

    match r {
        Ok(()) => ProcExit::from(ExitCode::Success as u8),
        Err(e) => {
            eprintln!("brain-capture: {e}");
            ProcExit::from(e.exit_code() as u8)
        }
    }
}

/// Milliseconds since the epoch.
fn now_ms() -> u64 {
    jiff::Timestamp::now().as_millisecond().max(0) as u64
}

fn paths() -> Result<StorePaths> {
    StorePaths::resolve()
        .ok_or_else(|| brain_store::Error::Usage("cannot work out where the store lives".into()))
}

/// Run the overlay: hold the socket, keep the window warm, wait for the chord.
fn run() -> Result<()> {
    let paths = paths()?;

    // Bound before the window is built. Two overlays would both answer the
    // chord, and the second would take it while the first kept running,
    // invisible, holding the writer lock.
    let listener = socket::Listener::bind(&paths.root)?;
    tracing::info!(socket = %listener.path().display(), "overlay listening");

    let (tx, rx) = std::sync::mpsc::channel();
    let waker = window::Waker::default();

    let fired = gesture(paths.ops.clone(), tx, waker.clone());

    // The mechanisms that deliver the chord to this process rather than by
    // spawning a client. Started before the window rather than after: a failure
    // here is a failure to register the gesture at all, and it should be
    // reported now rather than discovered later by pressing the chord.
    //
    // Both hold their registration for exactly as long as the process runs —
    // the X11 grab is released with the connection, the portal session with the
    // bus connection — which is why neither has a `--register` step that
    // outlives it.
    match hotkey::select(&hotkey::Host) {
        Mechanism::X11 => {
            let held = fired.clone();
            x11::hold(&DEFAULT_CHORD, move || held())?;
        }
        Mechanism::Portal => {
            let held = fired.clone();
            portal::hold(&DEFAULT_CHORD, move || held())?;
        }
        // The chord arrives on the socket instead, from the client the desktop
        // spawns. Nothing to start here.
        Mechanism::Gnome | Mechanism::None => {}
    }

    // The other half of the due-work rule (§8.7). Startup is the floor; this is
    // what makes a deadline land near its time. It has to be started here
    // because this is the process that is already running — which is the whole
    // reason the rule names it.
    let due_paths = paths.clone();
    std::thread::spawn(move || due::tick(due_paths));

    std::thread::spawn(move || {
        loop {
            match listener.next() {
                Ok(()) => {
                    if !fired() {
                        return;
                    }
                }
                Err(e) => {
                    tracing::warn!(%e, "capture socket failed");
                    return;
                }
            }
        }
    });

    window::run(paths, rx, waker).map_err(|e| brain_store::Error::Usage(format!("overlay: {e}")))
}

/// What happens when the chord fires, whichever mechanism delivered it.
///
/// Shared rather than duplicated per backend. Each of the three steps is one
/// somebody would forget on a second path — and the one that matters most is
/// the recording, because leaving it out makes `brain status` report a working
/// gesture as dead. Returns `false` once the window is gone, which is every
/// caller's cue to stop.
fn gesture(
    ops_path: std::path::PathBuf,
    tx: std::sync::mpsc::Sender<()>,
    waker: window::Waker,
) -> std::sync::Arc<dyn Fn() -> bool + Send + Sync> {
    std::sync::Arc::new(move || {
        // Recorded at the point the request actually arrived. Recording it
        // where the chord is *sent* would mark the gesture healthy in exactly
        // the case it is not: a client that runs and reaches nothing.
        if let Ok(ops) = brain_store::ops::Ops::open(&ops_path) {
            let _ = ops.gesture_fired(now_ms());
        }
        if tx.send(()).is_err() {
            return false;
        }
        // Wake the loop rather than letting it discover this on a timer. This
        // is the whole reason the process can sleep between presses.
        waker.wake();
        true
    })
}

/// Ask the resident overlay to appear. This is what the chord runs.
fn show() -> Result<()> {
    let paths = paths()?;
    if socket::signal(&paths.root)? {
        return Ok(());
    }
    // The chord is registered and nothing is listening. Exactly the
    // registered-but-dead state a spawn-based binding makes easy, so it says so
    // rather than exiting quietly and leaving the gesture looking broken.
    Err(brain_store::Error::Usage(
        "no overlay is running; start it with `brain-capture --run`".into(),
    ))
}

/// Arrange for the overlay to be running before the chord is ever pressed.
///
/// The desktop's own overview is the model: pressing Super reveals a surface
/// the shell built at login rather than launching one. Instant is what being
/// already there buys, and it cannot be bought any other way.
fn enable_autostart() -> Result<()> {
    let path = autostart::install()?;
    println!("The overlay will start at login.");
    println!("  {}", path.display());
    println!();
    println!("It waits with the window built and hidden, so the chord only has");
    println!("to reveal it. About 30 MB of memory, and no measurable processor");
    println!("time while it waits.");
    println!();
    println!("Start it now without logging out:");
    println!("  brain-capture --run &");
    Ok(())
}

fn disable_autostart() -> Result<()> {
    if autostart::remove()? {
        println!("It will no longer start at login.");
        println!("The chord will do nothing until something is running —");
        println!("`brain +` from a terminal is unaffected.");
    } else {
        println!("It was not set to start at login.");
    }
    Ok(())
}

/// Say what would happen, and what is in force.
fn report() -> Result<()> {
    let mechanism = hotkey::select(&hotkey::Host);
    println!("chord      {}", DEFAULT_CHORD.human());
    println!("mechanism  {}", mechanism.name());
    let running = paths()
        .map(|p| socket::signal(&p.root).unwrap_or(false))
        .unwrap_or(false);
    println!(
        "overlay    {}",
        if running { "running" } else { "not running" }
    );
    println!(
        "at login   {}",
        if autostart::installed() { "yes" } else { "no" }
    );
    if mechanism == Mechanism::None {
        println!();
        no_mechanism();
    }
    Ok(())
}

fn register() -> Result<()> {
    let mechanism = hotkey::select(&hotkey::Host);
    match mechanism {
        Mechanism::Gnome => {
            let command = client_command();
            let r = gnome::register(&gnome::Gsettings, &DEFAULT_CHORD, &command)?;
            println!("registered {} via {}", r.binding, mechanism.name());
            println!("  runs: {}", r.command);
            if let Some(conflict) = &r.conflict {
                // Reported, never taken. Silently stealing a chord the user set
                // deliberately would lose them a binding they rely on and say
                // nothing about it.
                println!();
                println!("Warning: '{conflict}' already uses this chord.");
                println!("One of them will win and it may not be this one.");
            }
            println!();
            confirm(&paths()?)
        }
        // Nothing to write, and that is the whole difference. The GNOME
        // mechanism registers a command with the desktop, which then exists
        // whether or not anything is listening; an X11 grab is held by the
        // running process and released with it. So there is no registration
        // step to perform here — only the same confirmation, which is what
        // actually answers the question the user asked.
        Mechanism::X11 => {
            println!(
                "registered {} via {}",
                DEFAULT_CHORD.human(),
                mechanism.name()
            );
            println!("  held by the overlay itself; nothing is written to your configuration");
            println!();
            println!("The grab lasts as long as the overlay runs. Start it at login with:");
            println!("  brain-capture --autostart");
            println!();
            confirm(&paths()?)
        }
        // The portal binds when the overlay starts, and the binding lives with
        // the session — so there is nothing to write here either, and the
        // desktop will ask the user rather than this program deciding. Saying
        // so matters: this is the one mechanism where the chord that ends up
        // bound may not be the one that was asked for.
        Mechanism::Portal => {
            println!(
                "registered {} via {}",
                DEFAULT_CHORD.human(),
                mechanism.name()
            );
            println!("  requested when the overlay starts; the desktop asks you to confirm it");
            println!("  nothing is written to your configuration");
            println!();
            println!("The binding lasts as long as the overlay runs. Start it at login with:");
            println!("  brain-capture --autostart");
            println!();
            println!("The portal may bind a different chord from the one requested.");
            println!("Whatever it binds is what the overlay reports on startup.");
            println!();
            confirm(&paths()?)
        }
        Mechanism::None => {
            no_mechanism();
            Ok(())
        }
    }
}

fn unregister() -> Result<()> {
    if hotkey::select(&hotkey::Host) == Mechanism::Gnome {
        gnome::unregister(&gnome::Gsettings)?;
        println!("removed the capture keybinding");
    }
    Ok(())
}

/// How long to wait for the chord during confirmation.
const CONFIRM_WAIT: std::time::Duration = std::time::Duration::from_secs(30);

/// Wait for the chord to actually arrive.
///
/// The step that turns "the setting was written" into "the gesture works".
/// Those are different claims, and only the second matters — a spawn-based
/// binding has several ways to be registered and inert, and every one of them
/// looks identical in the settings panel.
///
/// Not ceremonial. Registration reporting success while the chord reaches
/// nothing is the failure this whole path is most prone to, and the only way to
/// tell is to press it.
// A wall-clock read, and a deliberate one. The crate-wide ban exists so that
// the fold cannot consult a clock; this is a person being asked to press a key,
// and the elapsed time never reaches an event.
#[allow(
    clippy::disallowed_methods,
    clippy::disallowed_types,
    reason = "measuring how long to wait for a keypress, not stamping an event"
)]
fn confirm(paths: &StorePaths) -> Result<()> {
    let ops = brain_store::ops::Ops::open(&paths.ops)?;
    let before = ops.gesture_last_fired()?;

    println!("Press {} now.", DEFAULT_CHORD.human());
    println!(
        "(waiting {} seconds — Ctrl-C to skip)",
        CONFIRM_WAIT.as_secs()
    );

    let started = std::time::Instant::now();
    while started.elapsed() < CONFIRM_WAIT {
        std::thread::sleep(std::time::Duration::from_millis(200));
        if ops.gesture_last_fired()? != before {
            println!();
            println!("Confirmed — the chord reaches the overlay.");
            return Ok(());
        }
    }

    // Not an error. The binding is written and may well work once the overlay
    // is running; what cannot be claimed is that it has been seen to.
    println!();
    println!("Not seen. The binding is written, but nothing arrived.");
    println!();
    println!("Most likely: the overlay is not running.");
    println!("  brain-capture --run");
    println!();
    println!(
        "Otherwise something else may have taken {}.",
        DEFAULT_CHORD.human()
    );
    println!("`brain status` will keep reporting this until the chord is seen to work.");
    Ok(())
}

/// The command the desktop will run when the chord is pressed.
///
/// A thin client, not the overlay: it connects to the resident process and
/// exits. Spawning a whole application on a keypress is slow enough to feel,
/// and the gesture has to be instant or it stops being used.
fn client_command() -> String {
    let exe = std::env::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "brain-capture".into());
    format!("{exe} --show")
}

/// What to say when the desktop cannot register a global chord.
///
/// A supported situation rather than a failure, and it says so — the terminal
/// chord is unaffected, and on a platform that hides window identity it is the
/// better surface anyway.
fn no_mechanism() {
    println!("This desktop offers no way to register a global chord.");
    println!();
    println!("Capture from a terminal instead — the gesture is the same shape:");
    println!("  eval \"$(brain shell-init zsh)\"    then type, and press Ctrl-B");
}
