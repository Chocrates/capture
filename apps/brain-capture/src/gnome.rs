//! Registering the chord as a GNOME custom keybinding.
//!
//! # Why this exists at all
//!
//! GNOME 46 publishes `org.freedesktop.portal.Desktop` and does not implement
//! `GlobalShortcuts` on it. Wayland gives a client no way to grab a key itself.
//! What GNOME does offer is a list of custom keybindings in `gsettings`, each
//! one a command the shell runs when a chord is pressed — so the chord is
//! registered with the compositor, and the compositor starts a process.
//!
//! # The command has to be tiny
//!
//! Spawning a process is slow enough to notice, and the gesture has to feel
//! instant. So the registered command is not the overlay: it is a thin client
//! that connects to a unix socket, says "show yourself", and exits. The overlay
//! is already running with its window pre-warmed.
//!
//! That also makes the failure modes legible. A spawn-based binding has more
//! ways to be registered-but-not-firing than a grab does — the command can be
//! missing, the socket can be stale, the overlay can be dead — and each of them
//! looks identical from the settings panel, which will happily show the chord
//! bound to a command that does nothing.
//!
//! # Reading before writing
//!
//! GNOME stores custom keybindings as a list of relocatable schema paths, and
//! writing that list means replacing it. Appending without reading first would
//! silently delete every keybinding the user already had — which is somebody
//! else's configuration, destroyed by a tool that was only supposed to add one
//! line.

use std::process::Command;

use brain_store::{Error, Result};

use brain_store::hotkey::Chord;

/// Where GNOME keeps the list of custom keybindings.
const LIST_KEY: &str = "org.gnome.settings-daemon.plugins.media-keys custom-keybindings";

/// The relocatable schema each entry points at.
const ENTRY_SCHEMA: &str = "org.gnome.settings-daemon.plugins.media-keys.custom-keybinding";

/// The path this application uses. Fixed, so re-registering replaces rather
/// than accumulating.
const OUR_PATH: &str =
    "/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/brain-capture/";

/// What a registration attempt found.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Registration {
    /// The chord as GNOME will store it.
    pub binding: String,
    /// The command GNOME will run.
    pub command: String,
    /// A binding that already claims this chord, if there is one.
    pub conflict: Option<String>,
}

/// Somewhere `gsettings` values can be read and written.
///
/// A trait because the alternative is a test that edits the developer's own
/// desktop configuration.
pub trait Settings {
    fn get(&self, key: &str) -> Result<String>;
    fn set(&self, key: &str, value: &str) -> Result<()>;
    /// Every custom keybinding, as `(path, name, chord)`.
    ///
    /// The path is what identifies *our* entry. Trying to recognise it by its
    /// command instead is how the conflict check ends up asking whether our own
    /// command is ours — which it always is — and reporting nothing, ever.
    fn existing_bindings(&self) -> Result<Vec<(String, String, String)>>;
}

/// Install the keybinding, replacing any earlier one of ours.
pub fn register(s: &dyn Settings, chord: &Chord, command: &str) -> Result<Registration> {
    let binding = chord.gnome();

    // Reported, not resolved. Taking a chord away from something the user
    // deliberately set would be worse than not registering: they would lose a
    // binding they rely on and be told nothing.
    //
    // Our own earlier registration is excluded by **path**, which is the only
    // thing that reliably identifies it. An earlier version compared commands
    // and got the direction wrong — it asked whether the command being
    // registered was ours, which it always is, so no conflict was ever
    // reported. The unit test passed because it supplied a command no real
    // caller ever passes.
    let conflict = s
        .existing_bindings()?
        .into_iter()
        .find(|(path, _, b)| *b == binding && path != OUR_PATH)
        .map(|(_, name, _)| name);

    let list = s.get(LIST_KEY)?;
    let mut paths = parse_list(&list);
    if !paths.iter().any(|p| p == OUR_PATH) {
        // Appending to what is there. Writing a fresh list would delete every
        // keybinding the user already had.
        paths.push(OUR_PATH.to_string());
    }
    s.set(LIST_KEY, &render_list(&paths))?;

    let entry = format!("{ENTRY_SCHEMA}:{OUR_PATH}");
    s.set(&format!("{entry} name"), "'brain capture'")?;
    s.set(&format!("{entry} command"), &format!("'{command}'"))?;
    s.set(&format!("{entry} binding"), &format!("'{binding}'"))?;

    Ok(Registration {
        binding,
        command: command.to_string(),
        conflict,
    })
}

/// Remove the keybinding, leaving everything else alone.
pub fn unregister(s: &dyn Settings) -> Result<()> {
    let list = s.get(LIST_KEY)?;
    let paths: Vec<String> = parse_list(&list)
        .into_iter()
        .filter(|p| p != OUR_PATH)
        .collect();
    s.set(LIST_KEY, &render_list(&paths))
}

/// Read GNOME's list-of-strings form: `['a', 'b']`.
///
/// An empty list prints as `@as []` — a type annotation, then the brackets. So
/// the content is taken from between the brackets rather than by stripping them
/// off the ends, which on `@as []` would leave `@as [` and produce one entry
/// that is not a path. That entry gets written straight back on the next
/// registration, and the settings daemon rejects the whole list.
fn parse_list(raw: &str) -> Vec<String> {
    let raw = raw.trim();
    let inner = match (raw.find('['), raw.rfind(']')) {
        (Some(a), Some(b)) if b > a => &raw[a + 1..b],
        _ => "",
    };
    inner
        .split(',')
        .map(|s| s.trim().trim_matches('\'').trim_matches('"').to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

fn render_list(paths: &[String]) -> String {
    let items: Vec<String> = paths.iter().map(|p| format!("'{p}'")).collect();
    format!("[{}]", items.join(", "))
}

/// `gsettings`, as installed.
pub struct Gsettings;

impl Settings for Gsettings {
    fn get(&self, key: &str) -> Result<String> {
        let args: Vec<&str> = key.split_whitespace().collect();
        let out = Command::new("gsettings")
            .arg("get")
            .args(&args)
            .output()
            .map_err(|e| Error::Usage(format!("could not run gsettings: {e}")))?;
        if !out.status.success() {
            return Err(Error::Usage(format!(
                "gsettings get {key}: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            )));
        }
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    }

    fn set(&self, key: &str, value: &str) -> Result<()> {
        let args: Vec<&str> = key.split_whitespace().collect();
        let out = Command::new("gsettings")
            .arg("set")
            .args(&args)
            .arg(value)
            .output()
            .map_err(|e| Error::Usage(format!("could not run gsettings: {e}")))?;
        if !out.status.success() {
            return Err(Error::Usage(format!(
                "gsettings set {key}: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            )));
        }
        Ok(())
    }

    fn existing_bindings(&self) -> Result<Vec<(String, String, String)>> {
        // Only the custom ones. GNOME's built-in shortcuts live across dozens of
        // schemas, and enumerating them all to report a conflict would be slow
        // and still incomplete — the confirmation step is what actually proves
        // the chord arrives.
        let mut out = Vec::new();
        for path in parse_list(&self.get(LIST_KEY)?) {
            let entry = format!("{ENTRY_SCHEMA}:{path}");
            let name = self
                .get(&format!("{entry} name"))
                .unwrap_or_default()
                .trim_matches('\'')
                .to_string();
            let binding = self
                .get(&format!("{entry} binding"))
                .unwrap_or_default()
                .trim_matches('\'')
                .to_string();
            if !binding.is_empty() {
                out.push((path, name, binding));
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::BTreeMap;

    use brain_store::hotkey::DEFAULT_CHORD;

    #[derive(Default)]
    struct Fake {
        values: RefCell<BTreeMap<String, String>>,
        existing: Vec<(String, String, String)>,
    }

    impl Fake {
        fn with(list: &str) -> Fake {
            let f = Fake::default();
            f.values
                .borrow_mut()
                .insert(LIST_KEY.to_string(), list.to_string());
            f
        }
        fn value(&self, key: &str) -> String {
            self.values.borrow().get(key).cloned().unwrap_or_default()
        }
    }

    impl Settings for Fake {
        fn get(&self, key: &str) -> Result<String> {
            Ok(self.values.borrow().get(key).cloned().unwrap_or_default())
        }
        fn set(&self, key: &str, value: &str) -> Result<()> {
            self.values
                .borrow_mut()
                .insert(key.to_string(), value.to_string());
            Ok(())
        }
        fn existing_bindings(&self) -> Result<Vec<(String, String, String)>> {
            Ok(self.existing.clone())
        }
    }

    #[test]
    fn registering_adds_our_path_to_the_list() {
        let f = Fake::with("@as []");
        register(&f, &DEFAULT_CHORD, "/usr/bin/brain-capture --show").unwrap();
        assert!(f.value(LIST_KEY).contains("brain-capture"));
    }

    #[test]
    fn registering_keeps_every_binding_the_user_already_had() {
        // The destructive mistake this module is shaped to avoid: GNOME stores
        // the list as one value, so writing it means replacing it, and writing
        // a fresh list would delete somebody else's configuration.
        let f = Fake::with("['/org/gnome/…/custom-keybindings/custom0/', '/org/gnome/…/custom1/']");
        register(&f, &DEFAULT_CHORD, "brain-capture --show").unwrap();

        let list = f.value(LIST_KEY);
        assert!(list.contains("custom0"), "{list}");
        assert!(list.contains("custom1"), "{list}");
        assert!(list.contains("brain-capture"), "{list}");
    }

    #[test]
    fn registering_twice_does_not_accumulate() {
        let f = Fake::with("@as []");
        register(&f, &DEFAULT_CHORD, "brain-capture --show").unwrap();
        register(&f, &DEFAULT_CHORD, "brain-capture --show").unwrap();
        assert_eq!(f.value(LIST_KEY).matches("brain-capture").count(), 1);
    }

    #[test]
    fn the_stored_chord_is_the_one_definition() {
        let f = Fake::with("@as []");
        let r = register(&f, &DEFAULT_CHORD, "brain-capture --show").unwrap();
        assert_eq!(r.binding, DEFAULT_CHORD.gnome());
        assert!(
            f.value(&format!("{ENTRY_SCHEMA}:{OUR_PATH} binding"))
                .contains("<Control><Super>space")
        );
    }

    #[test]
    fn a_conflicting_chord_is_reported_and_not_taken() {
        // Reported rather than resolved. Taking a chord away from something the
        // user set deliberately would lose them a binding they rely on and tell
        // them nothing.
        let mut f = Fake::with("@as []");
        f.existing = vec![(
            "/org/gnome/…/custom0/".into(),
            "screenshot".into(),
            DEFAULT_CHORD.gnome(),
        )];
        // The command a real caller passes: our own. An earlier version
        // excluded conflicts whenever the command looked like ours, which made
        // this check dead in production while passing a test that supplied a
        // command nothing ever sends.
        let r = register(&f, &DEFAULT_CHORD, "brain-capture --show").unwrap();
        assert_eq!(r.conflict.as_deref(), Some("screenshot"));
    }

    #[test]
    fn our_own_earlier_registration_is_not_a_conflict() {
        let mut f = Fake::with("@as []");
        f.existing = vec![(
            OUR_PATH.into(),
            "brain capture".into(),
            DEFAULT_CHORD.gnome(),
        )];
        let r = register(&f, &DEFAULT_CHORD, "brain-capture --show").unwrap();
        assert_eq!(r.conflict, None, "our own entry is not a conflict");
    }

    #[test]
    fn unregistering_removes_only_ours() {
        let f = Fake::with("@as []");
        register(&f, &DEFAULT_CHORD, "brain-capture --show").unwrap();
        f.set(
            LIST_KEY,
            &format!("['/org/gnome/…/custom0/', '{OUR_PATH}']"),
        )
        .unwrap();

        unregister(&f).unwrap();
        let list = f.value(LIST_KEY);
        assert!(list.contains("custom0"), "{list}");
        assert!(!list.contains("brain-capture"), "{list}");
    }

    #[test]
    fn an_empty_list_is_read_as_empty_not_as_one_blank_entry() {
        // GNOME writes an empty list as `@as []`, which a naive split turns
        // into a single empty string — and that entry then gets written back
        // as `''`, which the settings daemon rejects.
        assert!(parse_list("@as []").is_empty());
        assert!(parse_list("[]").is_empty());
        assert!(parse_list("").is_empty());
    }

    #[test]
    fn a_list_round_trips() {
        let paths = vec!["/a/".to_string(), "/b/".to_string()];
        assert_eq!(parse_list(&render_list(&paths)), paths);
    }
}
