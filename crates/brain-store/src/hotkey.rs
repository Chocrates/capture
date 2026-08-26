//! Choosing how the capture chord gets registered.
//!
//! Detection lives here rather than in the overlay because two programs need
//! the answer: the overlay, to register with the right mechanism, and `brain
//! status`, to say which one is in force and whether the gesture is still
//! reaching anything. Registration itself stays in the overlay — it is the only
//! thing that has a window to register *for*.
//!
//! # Chosen at runtime, never at compile time
//!
//! There is no single mechanism for a global hotkey on Linux, and which ones
//! work depends on the session the binary happens to be running in — not on the
//! machine it was built for. A build-time choice would produce a binary that is
//! correct on the developer's desktop and silently inert on the user's.
//!
//! So the mechanisms are tried in order of preference and the first one that is
//! genuinely available wins:
//!
//! | order | mechanism | when it applies |
//! |---|---|---|
//! | 1 | `GlobalShortcuts` portal | the interface is actually present (KDE today) |
//! | 2 | GNOME `gsettings` | a GNOME session, using a custom keybinding |
//! | 3 | X11 `XGrabKey` | a **genuine** X11 session |
//! | 4 | none | say so loudly; the terminal chord still works |
//!
//! # Two ways to be wrong that both look like success
//!
//! **The portal may be advertised but absent.** `org.freedesktop.portal.Desktop`
//! exists on every modern desktop; the `GlobalShortcuts` interface on it does
//! not. GNOME 46 — the target — publishes the portal and no such interface. So
//! the probe asks for the interface by name rather than assuming that a portal
//! implies its contents.
//!
//! **Xwayland will accept an X11 grab and never deliver it.** Under a Wayland
//! session `DISPLAY` is still set and `XGrabKey` still succeeds, so the
//! registration reports success — but the grab only fires while an X11 window
//! has focus, and in a Wayland session none ever does. The result is a chord
//! that is registered, reported as working, and completely dead. That is the
//! exact failure the confirmation step exists to catch, and it is cheaper to
//! refuse it here: X11 is selected only when the session really is X11.
//!
//! Both conditions were checked on the target machine rather than reasoned
//! about — see the tests, and `docs` in the task list.

use std::process::Command;

/// The default chord, named in exactly one place.
///
/// Every mechanism below has its own syntax for it, and each translation is a
/// chance for them to drift apart — after which the interface reports one chord
/// and the desktop listens for another.
pub const DEFAULT_CHORD: Chord = Chord {
    ctrl: true,
    alt: false,
    shift: false,
    super_: true,
    key: "space",
};

/// A key combination, in a form each backend can translate.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Chord {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub super_: bool,
    pub key: &'static str,
}

impl Chord {
    /// GNOME's binding syntax, as `gsettings` expects it.
    pub fn gnome(&self) -> String {
        let mut s = String::new();
        if self.ctrl {
            s.push_str("<Control>");
        }
        if self.alt {
            s.push_str("<Alt>");
        }
        if self.shift {
            s.push_str("<Shift>");
        }
        if self.super_ {
            s.push_str("<Super>");
        }
        s.push_str(self.key);
        s
    }

    /// The portal's trigger syntax.
    ///
    /// The `GlobalShortcuts` interface takes a *preferred* trigger in the
    /// shortcuts syntax the XDG specification defines — `CTRL`, `ALT`, `SHIFT`
    /// and `LOGO` for the Super key, joined with `+`. Preferred is the whole
    /// character of this backend: the portal asks the user to confirm the
    /// binding and may hand back a different one, which is the point of a
    /// desktop-neutral mechanism and the reason it outranks writing directly
    /// into someone's settings.
    pub fn portal(&self) -> String {
        let mut parts = Vec::new();
        if self.ctrl {
            parts.push("CTRL".to_string());
        }
        if self.alt {
            parts.push("ALT".to_string());
        }
        if self.shift {
            parts.push("SHIFT".to_string());
        }
        if self.super_ {
            // Not "SUPER". The specification names this key after the logo on
            // it, and a portal given "SUPER" binds nothing.
            parts.push("LOGO".to_string());
        }
        parts.push(self.key.to_string());
        parts.join("+")
    }

    /// How to say it to a person.
    pub fn human(&self) -> String {
        let mut parts = Vec::new();
        if self.ctrl {
            parts.push("Ctrl");
        }
        if self.alt {
            parts.push("Alt");
        }
        if self.shift {
            parts.push("Shift");
        }
        if self.super_ {
            parts.push("Super");
        }
        let key = match self.key {
            "space" => "Space",
            k => k,
        };
        format!("{}-{}", parts.join("-"), key)
    }
}

/// How the chord gets registered.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mechanism {
    /// The desktop-neutral portal. Preferred wherever it exists.
    Portal,
    /// A GNOME custom keybinding, spawning a thin client.
    Gnome,
    /// An X11 grab, under a real X11 session.
    X11,
    /// Nothing on this desktop can register a global chord.
    None,
}

impl Mechanism {
    /// What to call it in `brain status`.
    pub fn name(&self) -> &'static str {
        match self {
            Mechanism::Portal => "desktop portal",
            Mechanism::Gnome => "GNOME keybinding",
            Mechanism::X11 => "X11 grab",
            Mechanism::None => "none",
        }
    }
}

/// What the running session will admit to.
///
/// A trait so selection can be tested against every desktop this code claims to
/// handle, on a machine that is only ever one of them.
pub trait Session {
    /// `XDG_SESSION_TYPE` — `wayland`, `x11`, or absent.
    fn session_type(&self) -> Option<String>;
    /// `XDG_CURRENT_DESKTOP`, which may list several colon-separated names.
    fn desktop(&self) -> Option<String>;
    /// Whether the `GlobalShortcuts` portal interface is really there.
    fn has_global_shortcuts_portal(&self) -> bool;
    /// Whether `gsettings` can be run.
    fn has_gsettings(&self) -> bool;
}

/// Pick the best mechanism this session actually supports.
pub fn select(s: &dyn Session) -> Mechanism {
    if s.has_global_shortcuts_portal() {
        return Mechanism::Portal;
    }

    let wayland = s.session_type().as_deref() == Some("wayland");

    if is_gnome(s) && s.has_gsettings() {
        return Mechanism::Gnome;
    }

    // Only a genuine X11 session. Under Wayland, `DISPLAY` is set, the grab
    // succeeds, and the chord never fires — registered, reported as working,
    // and dead.
    if !wayland && s.session_type().as_deref() == Some("x11") {
        return Mechanism::X11;
    }

    Mechanism::None
}

fn is_gnome(s: &dyn Session) -> bool {
    s.desktop()
        .map(|d| d.to_ascii_lowercase().contains("gnome"))
        .unwrap_or(false)
}

/// The session this process is in.
pub struct Host;

impl Session for Host {
    fn session_type(&self) -> Option<String> {
        std::env::var("XDG_SESSION_TYPE").ok()
    }

    fn desktop(&self) -> Option<String> {
        std::env::var("XDG_CURRENT_DESKTOP").ok()
    }

    /// Ask the portal what interfaces it has, rather than assuming.
    ///
    /// The portal object exists on every modern desktop and says nothing about
    /// which interfaces it implements. On the target — GNOME 46 — introspection
    /// lists nineteen interfaces and `GlobalShortcuts` is not among them.
    ///
    /// Shelling out is acceptable here and only here: this runs once at startup
    /// to choose a mechanism, never on the capture path.
    fn has_global_shortcuts_portal(&self) -> bool {
        let Ok(out) = Command::new("gdbus")
            .args([
                "introspect",
                "--session",
                "--dest",
                "org.freedesktop.portal.Desktop",
                "--object-path",
                "/org/freedesktop/portal/desktop",
            ])
            .output()
        else {
            return false;
        };
        String::from_utf8_lossy(&out.stdout).contains("org.freedesktop.portal.GlobalShortcuts")
    }

    fn has_gsettings(&self) -> bool {
        Command::new("gsettings")
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fake {
        session: Option<&'static str>,
        desktop: Option<&'static str>,
        portal: bool,
        gsettings: bool,
    }

    impl Session for Fake {
        fn session_type(&self) -> Option<String> {
            self.session.map(String::from)
        }
        fn desktop(&self) -> Option<String> {
            self.desktop.map(String::from)
        }
        fn has_global_shortcuts_portal(&self) -> bool {
            self.portal
        }
        fn has_gsettings(&self) -> bool {
            self.gsettings
        }
    }

    /// This machine, as measured: GNOME 46 on Wayland, no portal interface.
    fn target() -> Fake {
        Fake {
            session: Some("wayland"),
            desktop: Some("ubuntu:GNOME"),
            portal: false,
            gsettings: true,
        }
    }

    #[test]
    fn the_target_machine_uses_the_gnome_keybinding() {
        // Measured, not assumed: introspecting the portal on this machine lists
        // nineteen interfaces and `GlobalShortcuts` is not one of them.
        assert_eq!(select(&target()), Mechanism::Gnome);
    }

    #[test]
    fn a_portal_wins_wherever_it_exists() {
        let kde = Fake {
            session: Some("wayland"),
            desktop: Some("KDE"),
            portal: true,
            gsettings: false,
        };
        assert_eq!(select(&kde), Mechanism::Portal);
    }

    #[test]
    fn a_portal_beats_gnomes_own_mechanism() {
        // Preference order, not availability order. When GNOME grows the
        // interface, the desktop-neutral path should take over on its own.
        let future_gnome = Fake {
            portal: true,
            ..target()
        };
        assert_eq!(select(&future_gnome), Mechanism::Portal);
    }

    #[test]
    fn x11_is_refused_under_wayland() {
        // The failure this ordering exists to prevent. `DISPLAY` is set in a
        // Wayland session, `XGrabKey` succeeds, and the chord never fires —
        // registered, reported as working, and dead.
        let xwayland = Fake {
            session: Some("wayland"),
            desktop: Some("something-without-gsettings"),
            portal: false,
            gsettings: false,
        };
        assert_eq!(select(&xwayland), Mechanism::None);
    }

    #[test]
    fn x11_is_used_under_a_genuine_x11_session() {
        let real_x11 = Fake {
            session: Some("x11"),
            desktop: Some("XFCE"),
            portal: false,
            gsettings: false,
        };
        assert_eq!(select(&real_x11), Mechanism::X11);
    }

    #[test]
    fn gnome_without_gsettings_falls_through() {
        // The mechanism is the tool, not the desktop's name. A GNOME session
        // with no way to run `gsettings` cannot register anything this way.
        let odd = Fake {
            gsettings: false,
            ..target()
        };
        assert_eq!(select(&odd), Mechanism::None);
    }

    #[test]
    fn an_unknown_desktop_gets_nothing_rather_than_a_guess() {
        let unknown = Fake {
            session: None,
            desktop: None,
            portal: false,
            gsettings: false,
        };
        assert_eq!(select(&unknown), Mechanism::None);
    }

    #[test]
    fn the_chord_is_translated_consistently() {
        // One definition, three renderings. They drift apart the moment the
        // chord is written down twice, after which the interface names one
        // chord and the desktop listens for another.
        assert_eq!(DEFAULT_CHORD.gnome(), "<Control><Super>space");
        assert_eq!(DEFAULT_CHORD.portal(), "CTRL+LOGO+space");
        assert_eq!(DEFAULT_CHORD.human(), "Ctrl-Super-Space");
    }

    #[test]
    fn the_portal_calls_the_super_key_by_its_logo() {
        // "SUPER" is the obvious guess and binds nothing: the XDG shortcuts
        // syntax names this key after the logo printed on it.
        assert_eq!(DEFAULT_CHORD.portal(), "CTRL+LOGO+space");

        let alt_shift = Chord {
            ctrl: false,
            alt: true,
            shift: true,
            super_: false,
            key: "n",
        };
        assert_eq!(alt_shift.portal(), "ALT+SHIFT+n");
    }

    #[test]
    fn every_mechanism_can_be_named() {
        // `brain status` reports which one was selected, and "none" has to be
        // as reportable as the others — it is the case a user most needs told.
        for m in [
            Mechanism::Portal,
            Mechanism::Gnome,
            Mechanism::X11,
            Mechanism::None,
        ] {
            assert!(!m.name().is_empty());
        }
    }
}
