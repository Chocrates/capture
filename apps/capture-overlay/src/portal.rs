//! Registering the chord through the `GlobalShortcuts` portal.
//!
//! # The one mechanism that asks
//!
//! The other two backends take the chord. GNOME's writes a binding into the
//! user's settings; the X11 one grabs the key from the server. This one asks
//! the desktop, and the desktop asks the user — the portal shows its own dialog
//! and may hand back a *different* trigger from the one requested. That is why
//! it outranks both: it is the only path where a capture tool binding a global
//! key is something the person at the keyboard agreed to, and the only one that
//! cannot quietly take a chord out from under something else.
//!
//! So [`Chord::portal`] is a *preferred* trigger, not a demand, and the
//! shortcut that ends up bound is whatever comes back.
//!
//! # Why this needs a real D-Bus connection
//!
//! `hotkey.rs` probes for the interface by shelling out to `gdbus`, which is
//! fine for a question with a one-shot answer. This cannot work that way.
//!
//! The portal's methods do not return their results. `CreateSession` returns a
//! handle to a `Request` object, and the actual answer arrives later as a
//! `Response` signal on it — which means the subscription has to exist *before*
//! the call is made, or the reply is missed and the session never opens.
//! Activations then arrive as signals for as long as the process lives.
//!
//! # Unverified against a real portal
//!
//! Written against the specification and not yet run against a desktop that
//! implements it: the target machine is GNOME 46, which publishes the portal
//! and not this interface. Every part that can be checked without one is a
//! pure function below with tests. The D-Bus conversation itself has been
//! read, not exercised, and the first KDE session it meets is its first real
//! test.

use std::collections::HashMap;

use capture_store::hotkey::Chord;
use capture_store::{Error, Result};
use futures_lite::StreamExt;
use zbus::proxy::SignalStream;
use zbus::{Connection, OwnedObjectPath, OwnedVariant, Proxy, Variant};

/// Where the portal lives.
const DEST: &str = "org.freedesktop.portal.Desktop";
const OBJECT: &str = "/org/freedesktop/portal/desktop";
const SHORTCUTS: &str = "org.freedesktop.portal.GlobalShortcuts";
const REQUEST: &str = "org.freedesktop.portal.Request";

/// The one shortcut this application binds.
///
/// A single id, fixed. The portal remembers bindings per application and per
/// id, so changing this string would orphan whatever the user already agreed
/// to and ask them again.
const SHORTCUT_ID: &str = "capture";

/// Where the portal will deliver the reply to a call.
///
/// Derived rather than read from the method's return value, because the
/// subscription has to be in place before the call is made — a reply that
/// arrives first is simply lost. The rule comes from the specification: the
/// sender's unique name, without its leading colon and with every dot replaced
/// by an underscore, then the token.
pub fn request_path(unique_name: &str, token: &str) -> String {
    let sender = unique_name.trim_start_matches(':').replace('.', "_");
    format!("{OBJECT}/request/{sender}/{token}")
}

/// A token the portal will accept.
///
/// It becomes part of an object path, where only `[A-Za-z0-9_]` is legal.
/// Anything else in it produces a path the bus rejects, and the call fails in a
/// way that reads like the portal refusing the request.
pub fn token(kind: &str, n: u64) -> String {
    let kind: String = kind
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    format!("capture_overlay_{kind}_{n}")
}

/// Open a session, bind the chord, and answer it until the process ends.
///
/// `fired` is what the socket and X11 paths run too — showing the window is
/// only part of answering the chord; the press also has to be recorded, or the
/// dead-gesture check reports a working gesture as dead.
pub fn hold(chord: &Chord, fired: impl Fn() -> bool + Send + 'static) -> Result<()> {
    let (conn, mut activations) = zbus::block_on(open(chord))?;

    std::thread::spawn(move || {
        // The connection is moved in and held: dropping it would close the
        // session, and the portal would forget the binding.
        let _conn = conn;
        zbus::block_on(async {
            while let Some(message) = activations.next().await {
                // `(o session_handle, s shortcut_id, t timestamp, a{sv} options)`.
                // Only the id is inspected; the session is this process's own,
                // and the portal's timestamp is the desktop's clock rather than
                // the store's.
                let Ok((_, id, _, _)) = message.body().deserialize::<(
                    OwnedObjectPath,
                    String,
                    u64,
                    HashMap<String, OwnedVariant>,
                )>() else {
                    tracing::warn!("an activation arrived in a shape this does not understand");
                    continue;
                };

                if id != SHORTCUT_ID {
                    continue;
                }

                if !fired() {
                    return;
                }
            }
            tracing::warn!("the portal stopped sending activations; the chord is no longer bound");
        })
    });

    Ok(())
}

/// Open the session and bind the chord, then subscribe to its activations.
async fn open(chord: &Chord) -> Result<(Connection, SignalStream<'static>)> {
    let conn = Connection::session()
        .await
        .map_err(|e| dbus("cannot reach the session bus", e))?;

    let unique = conn
        .unique_name()
        .ok_or_else(|| Error::Usage("the session bus gave this process no name".into()))?
        .to_string();

    let session = create_session(&conn, &unique).await?;
    let trigger = bind(&conn, &unique, &session, chord).await?;

    // What the user agreed to, which is not necessarily what was asked for.
    // Reported rather than assumed: a portal that binds a different chord and a
    // program that keeps announcing the requested one is a program telling the
    // user the wrong thing to press.
    match &trigger {
        Some(t) if t != &chord.portal() => {
            tracing::warn!(
                requested = %chord.portal(),
                bound = %t,
                "the portal bound a different chord from the one requested"
            );
        }
        _ => tracing::info!(chord = %chord.human(), "the portal bound the chord"),
    }

    let shortcuts = Proxy::new(&conn, DEST, OBJECT, SHORTCUTS)
        .await
        .map_err(|e| dbus("cannot reach the shortcuts portal", e))?;

    let activations = shortcuts
        .receive_signal("Activated")
        .await
        .map_err(|e| dbus("cannot listen for shortcut activations", e))?;

    Ok((conn, activations))
}

/// Ask the portal for a session, and wait for it to say yes.
async fn create_session(conn: &Connection, unique: &str) -> Result<OwnedObjectPath> {
    let handle = token("session", 1);
    let session_token = token("shared", 1);

    // Subscribed before the call, which is the whole reason this is not a
    // `gdbus` one-liner.
    let mut replies = responses(conn, unique, &handle).await?;

    let options: HashMap<&str, Variant> = HashMap::from([
        ("handle_token", Variant::from(handle.as_str())),
        (
            "session_handle_token",
            Variant::from(session_token.as_str()),
        ),
    ]);

    let shortcuts = Proxy::new(conn, DEST, OBJECT, SHORTCUTS)
        .await
        .map_err(|e| dbus("cannot reach the shortcuts portal", e))?;
    shortcuts
        .call_method("CreateSession", &(options,))
        .await
        .map_err(|e| dbus("the portal refused to open a session", e))?;

    let results = wait(&mut replies, "opening a session").await?;

    // `session_handle` is a string in the results and an object path
    // everywhere it is then used. The specification is inconsistent about this
    // and implementations follow it, so both are accepted.
    let handle: String = match results.get("session_handle") {
        Some(v) => String::try_from(v.try_clone().map_err(variant)?)
            .or_else(|_| {
                v.try_clone()
                    .map_err(variant)
                    .and_then(|v| OwnedObjectPath::try_from(v).map_err(variant))
                    .map(|p| p.as_str().to_string())
            })
            .map_err(|_| Error::Usage("the portal's session handle is not a path".into()))?,
        None => {
            return Err(Error::Usage(
                "the portal opened a session with no handle".into(),
            ));
        }
    };

    OwnedObjectPath::try_from(handle)
        .map_err(|e| Error::Usage(format!("the portal's session handle is not a path: {e}")))
}

/// Ask for the chord, and report what was actually bound.
async fn bind(
    conn: &Connection,
    unique: &str,
    session: &OwnedObjectPath,
    chord: &Chord,
) -> Result<Option<String>> {
    let handle = token("bind", 1);
    let mut replies = responses(conn, unique, &handle).await?;

    let mut description: HashMap<&str, Variant> = HashMap::new();
    description.insert("description", Variant::from("Capture a note"));
    description.insert("preferred_trigger", Variant::from(chord.portal()));

    let shortcuts: Vec<(&str, HashMap<&str, Variant>)> = vec![(SHORTCUT_ID, description)];
    let options: HashMap<&str, Variant> =
        HashMap::from([("handle_token", Variant::from(handle.as_str()))]);

    let proxy = Proxy::new(conn, DEST, OBJECT, SHORTCUTS)
        .await
        .map_err(|e| dbus("cannot reach the shortcuts portal", e))?;

    // The empty parent window is deliberate: there is no window to parent the
    // portal's dialog to. The overlay's own window is hidden at this point and
    // showing it to ask about a keybinding would be the wrong surface entirely.
    proxy
        .call_method("BindShortcuts", &(session, shortcuts, "", options))
        .await
        .map_err(|e| dbus("the portal refused to bind the chord", e))?;

    let results = wait(&mut replies, "binding the chord").await?;
    Ok(bound_trigger(&results))
}

/// What the portal says it bound, if it says.
fn bound_trigger(results: &HashMap<String, OwnedVariant>) -> Option<String> {
    let shortcuts = results.get("shortcuts")?;
    let list: Vec<(String, HashMap<String, OwnedVariant>)> =
        Vec::try_from(shortcuts.try_clone().ok()?).ok()?;
    let (_, props) = list.into_iter().find(|(id, _)| id == SHORTCUT_ID)?;
    String::try_from(props.get("trigger_description")?.try_clone().ok()?).ok()
}

/// Subscribe to the replies for one request, before it is made.
async fn responses(conn: &Connection, unique: &str, handle: &str) -> Result<SignalStream<'static>> {
    let path = request_path(unique, handle);
    let request = Proxy::new(conn, DEST, path, REQUEST)
        .await
        .map_err(|e| dbus("cannot listen for the portal's reply", e))?;
    request
        .receive_signal("Response")
        .await
        .map_err(|e| dbus("cannot listen for the portal's reply", e))
}

/// Wait for one reply, and refuse anything that is not a plain yes.
///
/// Response code 0 is success, 1 is the user cancelling, 2 is the portal ending
/// the interaction some other way. Cancelling is a decision rather than a fault
/// and says so — the whole point of this backend is that it is the user's call.
async fn wait(
    replies: &mut SignalStream<'_>,
    doing: &str,
) -> Result<HashMap<String, OwnedVariant>> {
    let message = replies
        .next()
        .await
        .ok_or_else(|| Error::Usage(format!("the portal never replied while {doing}")))?;

    let (code, results): (u32, HashMap<String, OwnedVariant>) =
        message.body().deserialize().map_err(variant)?;

    match code {
        0 => Ok(results),
        1 => Err(Error::Usage(format!(
            "the chord was not bound: the request was declined while {doing}"
        ))),
        n => Err(Error::Usage(format!(
            "the portal ended the request while {doing} (code {n})"
        ))),
    }
}

fn dbus(what: &str, e: zbus::Error) -> Error {
    Error::Usage(format!("{what}: {e}"))
}

fn variant(e: impl std::fmt::Display) -> Error {
    Error::Usage(format!(
        "the portal replied in a shape this does not understand: {e}"
    ))
}

// Linux mechanisms, never selected on a Mac, where the default chord is
// a different one and these assertions are about this one.
#[cfg(all(test, not(target_os = "macos")))]
mod tests {
    use super::*;
    use capture_store::hotkey::DEFAULT_CHORD;

    #[test]
    fn the_reply_path_is_derived_the_way_the_portal_builds_it() {
        // The subscription has to be in place before the call, so this path is
        // computed rather than read back. Getting it wrong loses the reply and
        // the session simply never opens — with no error, because nothing
        // failed.
        assert_eq!(
            request_path(":1.192", "capture_overlay_session_1"),
            "/org/freedesktop/portal/desktop/request/1_192/capture_overlay_session_1"
        );
    }

    #[test]
    fn a_name_without_a_leading_colon_is_handled_too() {
        assert_eq!(
            request_path("1.42", "t"),
            "/org/freedesktop/portal/desktop/request/1_42/t"
        );
    }

    #[test]
    fn a_token_is_always_a_legal_path_element() {
        // Only [A-Za-z0-9_] is legal in an object path element. Anything else
        // makes the bus reject the call, which reads like the portal refusing
        // the request rather than like a malformed token.
        for kind in ["session", "bind", "with-dash", "with.dot", "with space"] {
            let t = token(kind, 7);
            assert!(
                t.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'),
                "{t:?} is not a legal path element"
            );
        }
    }

    #[test]
    fn the_two_tokens_of_one_call_are_different() {
        // The request handle and the session handle are separate objects, and
        // giving them the same token points them at the same path.
        assert_ne!(token("session", 1), token("shared", 1));
    }

    #[test]
    fn the_preferred_trigger_is_what_the_portal_expects() {
        // Rendered by the chord itself so it cannot drift from the other two
        // backends' spellings of the same key combination.
        assert_eq!(DEFAULT_CHORD.portal(), "CTRL+LOGO+space");
    }

    #[test]
    fn a_reply_with_no_shortcuts_is_not_an_error() {
        // The portal is allowed to bind the chord and say nothing about what it
        // bound. That is a missing detail, not a failure, and treating it as
        // one would refuse a working binding.
        assert_eq!(bound_trigger(&HashMap::new()), None);
    }
}
