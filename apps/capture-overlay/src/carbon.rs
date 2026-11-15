//! The chord on a Mac: Carbon's `RegisterEventHotKey`, through `global-hotkey`
//! (it-runs-on-a-mac D1).
//!
//! # Why this one
//!
//! It is the only global-hotkey API on macOS that needs no privacy grant —
//! not Accessibility, not Input Monitoring. Those grants are tied to the
//! binary's signature, and an unsigned build loses them on every rebuild; a
//! chord that silently dies with each build is the failure §8 feared. A tap
//! (`CGEventTap`, `NSEvent` monitors) would need exactly that grant.
//!
//! # Threads
//!
//! The manager has to be made on the main thread, and the events arrive
//! through the main thread's run loop — which eframe's event loop runs. So
//! `hold` is called from `main` before the window starts, the manager is kept
//! for the life of the process, and a thread waits on the event channel.

use capture_store::hotkey::Chord;
use capture_store::{Error, Result};

#[cfg(target_os = "macos")]
pub fn hold(chord: &Chord, fired: impl Fn() -> bool + Send + 'static) -> Result<()> {
    use global_hotkey::hotkey::{Code, HotKey, Modifiers};
    use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};

    let manager = GlobalHotKeyManager::new()
        .map_err(|e| Error::Usage(format!("cannot reach macOS's hotkey service: {e}")))?;

    let code = match chord.key {
        "space" => Code::Space,
        other => {
            return Err(Error::Usage(format!(
                "no Mac key named {other:?} is known to the overlay"
            )));
        }
    };
    let mut mods = Modifiers::empty();
    if chord.ctrl {
        mods |= Modifiers::CONTROL;
    }
    if chord.alt {
        mods |= Modifiers::ALT;
    }
    if chord.shift {
        mods |= Modifiers::SHIFT;
    }
    if chord.super_ {
        mods |= Modifiers::META;
    }
    let key = HotKey::new(Some(mods), code);

    // Loud, and pointing somewhere that works (§8.9). A chord another
    // application holds cannot be taken, and pretending otherwise leaves a
    // gesture that does nothing.
    manager.register(key).map_err(|e| {
        Error::Usage(format!(
            "{} could not be registered — another application may hold it ({e}).\n\
             Capture from the command line meanwhile:  capture + <text>",
            chord.human()
        ))
    })?;

    // Kept for the life of the process: dropping it unregisters the chord.
    std::mem::forget(manager);

    let id = key.id();
    std::thread::spawn(move || {
        let events = GlobalHotKeyEvent::receiver();
        while let Ok(e) = events.recv() {
            if e.id == id && e.state == HotKeyState::Pressed && !fired() {
                return;
            }
        }
    });
    Ok(())
}

/// Never selected off a Mac; here so the overlay's match compiles everywhere.
#[cfg(not(target_os = "macos"))]
pub fn hold(_chord: &Chord, _fired: impl Fn() -> bool + Send + 'static) -> Result<()> {
    Err(Error::Usage(
        "the Carbon hotkey exists only on macOS".into(),
    ))
}
