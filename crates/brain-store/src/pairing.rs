//! The store's half of pairing, with no surface attached.
//!
//! Both the command line and the window admit devices, and both have to do
//! exactly the same thing: hold a socket open for a bounded window, run the
//! exchange, and write a `device_add` only once the other side has proved it
//! knew the code. Two copies of that would be two sets of rules about when the
//! admission is written — and the second set is the one that is subtly wrong,
//! because it is the one nobody exercises until somebody pairs a device from
//! the window.
//!
//! So the flow lives here and the surfaces differ only in how they say things.

use std::net::TcpListener;
use std::time::Duration;

use brain_sync::pairing::{Grant, Offer, admit, code_from, join};
use brain_sync::tcp::Tcp;

use crate::identity::Identity;
use crate::ops::Ops;
use crate::paths::StorePaths;
use crate::session::{Admitted, Session};
use crate::{Error, Result};

/// The port the pairing half listens on.
///
/// Not the sync port: pairing is a separate, deliberate exchange with a window
/// that closes, and running it where a daemon is already bound would mean one
/// could not happen while the other was.
pub const PORT: u16 = 41823;

/// How long the window stays open, in milliseconds.
///
/// Long enough to walk to the other device, short enough that a code left on a
/// screen is not an open door. It closes on the first failed attempt in any
/// case: a million online guesses is a different proposition from a million
/// offline ones, and only while the code stands still.
pub const WINDOW_MS: u64 = 180 * 1000;

/// A code, and the exchange it is waiting for.
pub struct Offering {
    /// The digits to show. Six of them, per §5.3's replacement gesture.
    pub code: Vec<u8>,
    listener: TcpListener,
}

impl Offering {
    /// Mint a code and start listening.
    pub fn open(paths: &StorePaths) -> Result<Self> {
        let code = code_from(crate::entropy::pairing_entropy()?);
        let listener = TcpListener::bind(("0.0.0.0", PORT))?;
        listener.set_nonblocking(true)?;
        let _ = paths;
        Ok(Self { code, listener })
    }

    /// The code as a person reads it.
    pub fn shown(&self) -> String {
        String::from_utf8_lossy(&self.code).to_string()
    }

    /// Wait for a device to answer, and admit it if it knew the code.
    ///
    /// `now` is supplied so the deadline is testable without waiting three
    /// minutes for it.
    pub fn admit_one(
        self,
        paths: &StorePaths,
        started: u64,
        now: impl Fn() -> u64,
    ) -> Result<[u8; 32]> {
        let identity = Identity::load(paths)?;

        let stream = loop {
            match self.listener.accept() {
                Ok((s, _)) => break s,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    if now().saturating_sub(started) > WINDOW_MS {
                        return Err(Error::Usage(
                            "nobody answered before the code expired".into(),
                        ));
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
                Err(e) => return Err(Error::Io(e)),
            }
        };
        stream.set_nonblocking(false)?;

        let mine = grant_from(paths, &identity)?;
        let offered = admit(Tcp::new(stream)?, &self.code, |_| mine.clone())
            .map_err(|e| Error::Usage(format!("{e}")))?;

        // Written only now. The exchange completing is what proves the other
        // side knew the code, and admitting before that would admit whoever
        // connected.
        let subject = Admitted {
            pubkey: offered.pubkey,
            incarnation: offered.incarnation,
            noise: offered.noise,
        };
        Session::open(paths.clone())?.admit(&subject)?;

        // What makes "a device was added that you did not expect" computable.
        // Without it every admission in the log looks alike, including the ones
        // this device performed itself.
        let ops = Ops::open(&paths.ops)?;
        ops.paired(&offered.pubkey, now())?;
        ops.learned_noise(&offered.pubkey, &offered.noise)?;

        Ok(offered.pubkey)
    }
}

/// Type a code shown on another device, and join its roster.
pub fn accept_code(paths: &StorePaths, digits: &[u8], addr: &str, now: u64) -> Result<Grant> {
    let identity = Identity::load(paths)?;

    let offer = Offer {
        pubkey: identity.public().to_bytes(),
        incarnation: identity.incarnation().0,
        noise: identity.noise_public(),
    };
    let grant =
        join(Tcp::connect(addr)?, digits, &offer).map_err(|e| Error::Usage(format!("{e}")))?;

    let ops = Ops::open(&paths.ops)?;
    ops.paired(&grant.pubkey, now)?;
    // The address a *sync* will use, not the one pairing just used. They are
    // different ports, and recording the pairing port would leave a remembered
    // address that never answers.
    let host = addr.split(':').next().unwrap_or("127.0.0.1");
    ops.peer_reached(
        &grant.pubkey,
        &format!("{host}:{}", brain_sync::discovery::sync_port()),
        now,
    )?;
    // Without this the newcomer cannot dial anybody: opening a session needs
    // the peer's X25519 key, that key normally comes from the log, and the log
    // is the thing it has not got yet.
    ops.learned_noise(&grant.pubkey, &grant.noise)?;

    // The other half of the mutual exchange. Each device has a root of its own
    // and only one survives the merge, so a device that was only ever admitted
    // may find its admitter is not a member of the combined roster — leaving
    // everything it writes unverifiable here, about half the time.
    if let Some(incarnation) = grant.incarnation {
        Session::open(paths.clone())?.admit(&Admitted {
            pubkey: grant.pubkey,
            incarnation,
            noise: grant.noise,
        })?;
    }

    Ok(grant)
}

/// What this device hands a newcomer.
fn grant_from(paths: &StorePaths, identity: &Identity) -> Result<Grant> {
    let log = crate::log::Log::open_read(&paths.events)?;
    let roster = brain_core::roster::build(&log.all()?);

    Ok(Grant {
        pubkey: identity.public().to_bytes(),
        noise: identity.noise_public(),
        members: roster.keys.iter().map(|k| (*k).into()).collect(),
        secret: roster.secret,
        incarnation: Some(identity.incarnation().0),
    })
}
