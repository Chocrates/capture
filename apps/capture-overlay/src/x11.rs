//! Registering the chord as an X11 passive grab.
//!
//! # Only under a session that is really X11
//!
//! This backend is the one with a silent failure mode, and it is worth being
//! precise about it. Under a Wayland session `DISPLAY` is still set, XWayland
//! answers, `GrabKey` succeeds and reports no error — and the chord never
//! fires, because a passive grab on the X root window only delivers while an
//! X11 window has focus, and in a Wayland session none ever does.
//!
//! The result is a gesture that is registered, reported as working, and dead.
//! `hotkey::select` already refuses to choose X11 outside a genuine X11
//! session; [`refuse_under_wayland`] checks again here rather than trusting
//! that, because the cost of the check is one environment variable and the cost
//! of being wrong is a capture surface that never appears.
//!
//! # Why a grab rather than a spawned command
//!
//! Unlike the GNOME backend, nothing is written to anyone's configuration and
//! no process is spawned: the grab lives inside the running overlay, and the
//! keypress arrives on a socket this process already owns. That removes every
//! registered-but-inert failure the spawn path has — there is no command to go
//! missing and no stale socket — and replaces them with one: the grab is only
//! held while the overlay runs.
//!
//! # Lock modifiers, which are not optional
//!
//! X11 reports Num Lock and Caps Lock in the same modifier field as Control and
//! Super, and a passive grab matches the modifier state *exactly*. A grab for
//! Ctrl-Super-Space is therefore not delivered when Num Lock happens to be on —
//! a chord that works until someone touches a key that is not part of it. So
//! the same chord is grabbed once per combination of the lock modifiers.

use capture_store::hotkey::Chord;
use capture_store::{Error, Result};
use x11rb::connection::Connection;
use x11rb::protocol::Event;
use x11rb::protocol::xproto::{ConnectionExt as _, GrabMode, ModMask};

/// The lock modifiers, which appear in the state whether or not they are part
/// of the chord.
///
/// Caps Lock is `LOCK`; Num Lock is conventionally `M2`. Both have to be
/// grabbed in every combination, or the chord silently stops working the moment
/// one of them is on.
const LOCKS: [u16; 4] = [
    0,
    1 << 1, // Caps Lock — ModMask::LOCK
    1 << 4, // Num Lock, by convention — ModMask::M2
    (1 << 1) | (1 << 4),
];

/// The X11 keysym for the chord's key.
///
/// Deliberately tiny: the default chord is Space and this is not a keyboard
/// layout engine. A key that is not listed is a key this backend refuses to
/// grab, which is a better answer than grabbing the wrong one — a passive grab
/// on the wrong keycode is another chord that reports success and never fires.
pub fn keysym(key: &str) -> Option<u32> {
    // Latin-1 keysyms are their own character codes, which covers the letters
    // and digits without a table. Everything else is named explicitly or
    // refused.
    let mut chars = key.chars();
    if let (Some(c), None) = (chars.next(), chars.next())
        && c.is_ascii_alphanumeric()
    {
        return Some(c.to_ascii_lowercase() as u32);
    }

    Some(match key {
        "space" => 0x0020,
        "Return" | "return" => 0xff0d,
        "Tab" | "tab" => 0xff09,
        _ => return None,
    })
}

/// The modifier mask for a chord, as X11 describes it.
///
/// Super is `M4` and Alt is `M1` by the convention every desktop follows. They
/// are conventions rather than guarantees — a remapped keyboard can move them —
/// which is one more reason this backend sits below the portal and GNOME in the
/// order.
pub fn modifiers(chord: &Chord) -> ModMask {
    let mut m = 0u16;
    if chord.ctrl {
        m |= ModMask::CONTROL.bits();
    }
    if chord.shift {
        m |= ModMask::SHIFT.bits();
    }
    if chord.alt {
        m |= ModMask::M1.bits();
    }
    if chord.super_ {
        m |= ModMask::M4.bits();
    }
    ModMask::from(m)
}

/// Every modifier combination the chord has to be grabbed under.
pub fn grab_masks(chord: &Chord) -> Vec<ModMask> {
    let base = modifiers(chord).bits();
    LOCKS
        .iter()
        .map(|lock| ModMask::from(base | lock))
        .collect()
}

/// Whether the state on a keypress is this chord, ignoring the lock modifiers.
///
/// The grab matches exactly, so this cannot disagree with it — but the event
/// still arrives with whatever lock bits were set, and comparing the raw state
/// against the chord would reject the press the grab just delivered.
pub fn is_chord(chord: &Chord, state: u16) -> bool {
    let locks = ModMask::LOCK.bits() | ModMask::M2.bits();
    state & !locks == modifiers(chord).bits()
}

/// Refuse to grab under Wayland, whatever `DISPLAY` says.
///
/// The second of two checks; `hotkey::select` is the first. This one exists
/// because the failure is silent and permanent: XWayland accepts the grab, the
/// registration reports success, and the chord is dead.
///
/// Takes the session as values rather than reading the environment, so the rule
/// can be tested for every session this code claims to handle — the same reason
/// `hotkey::Session` is a trait. A test that set these variables for real would
/// be setting them for every other test in the binary at the same time.
pub fn refuse_under_wayland(
    session_type: Option<&str>,
    wayland_display: Option<&str>,
) -> Result<()> {
    let wayland = session_type == Some("wayland") || wayland_display.is_some_and(|d| !d.is_empty());

    if wayland {
        return Err(Error::Usage(
            "this is a Wayland session; an X11 grab would be accepted and never fire".into(),
        ));
    }
    Ok(())
}

/// The same check, against the session this process is actually in.
fn refuse_here() -> Result<()> {
    let session = std::env::var("XDG_SESSION_TYPE").ok();
    let wayland = std::env::var("WAYLAND_DISPLAY").ok();
    refuse_under_wayland(session.as_deref(), wayland.as_deref())
}

/// Take the grab and answer it for as long as the process lives.
///
/// Returns once the grab is held, having handed the watching to a thread.
///
/// `fired` is what the socket path runs too, passed in rather than reproduced:
/// showing the window is only part of answering the chord — the press also has
/// to be recorded, or the dead-gesture check reports a working gesture as dead.
/// Getting that wrong on one path only would produce a warning that appears on
/// X11 and nowhere else. It returns `false` when there is nothing left to tell,
/// which is this thread's cue to stop.
pub fn hold(chord: &Chord, fired: impl Fn() -> bool + Send + 'static) -> Result<()> {
    refuse_here()?;

    let key = keysym(chord.key).ok_or_else(|| {
        Error::Usage(format!(
            "no X11 keysym is known for '{}'; refusing to grab a key it might not be",
            chord.key
        ))
    })?;

    let (conn, screen) = x11rb::connect(None)
        .map_err(|e| Error::Usage(format!("cannot reach the X server: {e}")))?;
    let root = conn.setup().roots[screen].root;

    let keycode = keycode_for(&conn, key)?;
    let masks = grab_masks(chord);

    for mask in &masks {
        conn.grab_key(
            // The grab is owned by the root window rather than passed on: the
            // chord is ours the moment it is pressed, and letting it through as
            // well would type a space into whatever was focused.
            false,
            root,
            *mask,
            keycode,
            GrabMode::ASYNC,
            GrabMode::ASYNC,
        )
        .map_err(|e| Error::Usage(format!("could not grab {}: {e}", chord.human())))?;
    }

    conn.flush()
        .map_err(|e| Error::Usage(format!("could not talk to the X server: {e}")))?;

    tracing::info!(
        chord = %chord.human(),
        grabs = masks.len(),
        "holding the X11 grab"
    );

    let chord = *chord;
    std::thread::spawn(move || {
        loop {
            match conn.wait_for_event() {
                Ok(Event::KeyPress(e))
                    if e.detail == keycode && is_chord(&chord, e.state.bits()) =>
                {
                    if !fired() {
                        return;
                    }
                }
                Ok(_) => {}
                Err(e) => {
                    // The connection is gone, which on X11 usually means the
                    // session is going with it. Said once, then the thread
                    // stops rather than spinning on a dead socket.
                    tracing::warn!(%e, "the X server connection ended; the grab is released");
                    return;
                }
            }
        }
    });

    Ok(())
}

/// Which physical key currently produces this keysym.
///
/// Asked of the server rather than assumed, because the mapping is the user's
/// and changes when they change layout. Only the first column of each keycode
/// is considered: the chord is an unshifted key by construction, and matching a
/// shifted column would grab a key that produces the symbol only with Shift
/// held, which is not the same chord.
fn keycode_for(conn: &impl Connection, keysym: u32) -> Result<u8> {
    let setup = conn.setup();
    let first = setup.min_keycode;
    let count = setup.max_keycode - setup.min_keycode + 1;

    let mapping = conn
        .get_keyboard_mapping(first, count)
        .map_err(|e| Error::Usage(format!("could not read the keyboard mapping: {e}")))?
        .reply()
        .map_err(|e| Error::Usage(format!("could not read the keyboard mapping: {e}")))?;

    let per = mapping.keysyms_per_keycode as usize;
    for (i, chunk) in mapping.keysyms.chunks(per).enumerate() {
        if chunk.first() == Some(&keysym) {
            return Ok(first + i as u8);
        }
    }

    Err(Error::Usage(format!(
        "this keyboard has no key producing keysym {keysym:#x}"
    )))
}

// Linux mechanisms, never selected on a Mac, where the default chord is
// a different one and these assertions are about this one.
#[cfg(all(test, not(target_os = "macos")))]
mod tests {
    use super::*;
    use capture_store::hotkey::DEFAULT_CHORD;

    #[test]
    fn the_default_chord_is_control_and_super() {
        // Super is M4 and Control is bit 2, by the convention every desktop
        // follows. Written down so a change to the default chord that this
        // backend cannot express fails here rather than in the field.
        let m = modifiers(&DEFAULT_CHORD).bits();
        assert_eq!(m, ModMask::CONTROL.bits() | ModMask::M4.bits());
    }

    #[test]
    fn every_lock_combination_is_grabbed() {
        // The bug this prevents: a chord that works until Num Lock is on. X11
        // reports the lock modifiers in the same field and a passive grab
        // matches exactly, so each combination is a separate grab.
        let masks = grab_masks(&DEFAULT_CHORD);
        assert_eq!(masks.len(), 4, "one grab per lock combination");

        let base = modifiers(&DEFAULT_CHORD).bits();
        for mask in &masks {
            assert_eq!(
                mask.bits() & base,
                base,
                "a grab dropped part of the chord itself"
            );
        }

        let mut seen: Vec<u16> = masks.iter().map(|m| m.bits()).collect();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), 4, "two grabs cover the same state");
    }

    #[test]
    fn a_press_with_num_lock_on_is_still_the_chord() {
        // The other half of the same bug. The grab delivers the event, and the
        // state still carries whatever lock bits were set at the time.
        let base = modifiers(&DEFAULT_CHORD).bits();
        assert!(is_chord(&DEFAULT_CHORD, base));
        assert!(is_chord(&DEFAULT_CHORD, base | ModMask::M2.bits()));
        assert!(is_chord(&DEFAULT_CHORD, base | ModMask::LOCK.bits()));
    }

    #[test]
    fn a_press_with_an_extra_modifier_is_not_the_chord() {
        // Shift-Ctrl-Super-Space is somebody else's chord.
        let base = modifiers(&DEFAULT_CHORD).bits();
        assert!(!is_chord(&DEFAULT_CHORD, base | ModMask::SHIFT.bits()));
        assert!(!is_chord(&DEFAULT_CHORD, base | ModMask::M1.bits()));
    }

    #[test]
    fn a_key_with_no_keysym_is_refused_rather_than_guessed() {
        // Grabbing the wrong keycode produces the same silent failure as
        // grabbing under Xwayland: registration succeeds, nothing fires.
        assert_eq!(keysym("space"), Some(0x0020));
        assert_eq!(keysym("F13"), None);
        assert_eq!(keysym(""), None);
    }

    #[test]
    fn wayland_is_refused_however_it_announces_itself() {
        // The silent failure this whole module is careful about. Both signals
        // are checked because either can be the only one present: a session
        // type of "wayland" with no WAYLAND_DISPLAY exported into this process,
        // or a socket with the type unset.
        assert!(refuse_under_wayland(Some("wayland"), Some("wayland-0")).is_err());
        assert!(refuse_under_wayland(Some("wayland"), None).is_err());
        assert!(refuse_under_wayland(None, Some("wayland-0")).is_err());
    }

    #[test]
    fn a_genuine_x11_session_is_allowed() {
        assert!(refuse_under_wayland(Some("x11"), None).is_ok());
        // An empty WAYLAND_DISPLAY is not a Wayland session. Treating a set but
        // empty variable as one would refuse the grab on machines where
        // something has exported it blank.
        assert!(refuse_under_wayland(Some("x11"), Some("")).is_ok());
    }

    #[test]
    fn an_unknown_session_is_allowed_to_try() {
        // Neither variable set. This is not Wayland by any available evidence,
        // and `hotkey::select` has already declined to pick X11 unless the
        // session said "x11" — so reaching here at all means something asked
        // for it deliberately.
        assert!(refuse_under_wayland(None, None).is_ok());
    }
}
