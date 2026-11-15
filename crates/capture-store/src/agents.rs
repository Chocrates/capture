//! Agents: programs that write notes under their own origin
//! (an-agent-keeps-notes-through-the-cli D1).
//!
//! # An agent is a key, not a flag
//!
//! Every event is signed by an origin, and an origin is a key. So "this was
//! written by Claude" is said the one way nothing can fake: the event is signed
//! by a key that only the agent's file holds, enrolled in the roster with the
//! role `agent`. A writer without the file cannot sign as the agent, and a
//! writer with it cannot sign as the person. Retiring the key stops it, exactly
//! as retiring a lost phone does.
//!
//! # Its own sequence, read from the log
//!
//! An agent's events have their own `seq` and chain, like any origin's. The
//! next number is read from the log, as the device's is. The one hazard is the
//! one a device has too: a log restored from a backup older than the agent's
//! last write would hand out positions peers already hold. So a small
//! watermark beside the key records how far the agent got, and a log behind it
//! refuses the write rather than reusing a number.
//!
//! # What an agent may not do
//!
//! Change membership (the roster refuses it), or store text that looks like a
//! credential (refused before signing, D3). Everything else an agent writes is
//! an ordinary event.

use std::fs;
use std::path::PathBuf;

use capture_core::{Dev, Incarnation};
use ed25519_dalek::SigningKey;
use minicbor::{Decode, Encode};

use crate::{Error, Result, StorePaths};

const AGENT_FILE_VERSION: u16 = 1;

/// The longest name an agent may have.
pub const NAME_MAX: usize = 32;

#[derive(Encode, Decode)]
struct AgentFile {
    #[n(0)]
    version: u16,
    #[cbor(n(1), with = "minicbor::bytes")]
    secret: [u8; 32],
    #[cbor(n(2), with = "minicbor::bytes")]
    incarnation: [u8; 16],
}

/// An agent's signing identity, loaded from its file.
pub struct AgentKey {
    /// What it was enrolled as.
    pub name: String,
    signing: SigningKey,
    incarnation: Incarnation,
    dev: Dev,
}

impl AgentKey {
    /// The origin its events are written under.
    pub fn dev(&self) -> Dev {
        self.dev
    }

    /// The key it is a roster member under.
    pub fn public(&self) -> [u8; 32] {
        self.signing.verifying_key().to_bytes()
    }

    /// The incarnation its origin derives from.
    pub fn incarnation(&self) -> Incarnation {
        self.incarnation
    }

    pub(crate) fn signing(&self) -> &SigningKey {
        &self.signing
    }
}

/// One enrolled agent, as the roster sees it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Agent {
    /// The name it writes under.
    pub name: String,
    /// Its roster key.
    pub pubkey: [u8; 32],
    /// Whether it has been cut off.
    pub retired: bool,
    /// Whether this device holds its key, and so can write as it.
    pub here: bool,
}

/// Refuse a name that would not make a safe file name or a readable label.
pub fn check_name(name: &str) -> Result<()> {
    let ok = !name.is_empty()
        && name.len() <= NAME_MAX
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && !name.starts_with('-');
    if ok {
        Ok(())
    } else {
        Err(Error::Usage(format!(
            "an agent's name is 1 to {NAME_MAX} lowercase letters, digits and dashes, \
             not starting with a dash: {name:?}"
        )))
    }
}

fn key_path(paths: &StorePaths, name: &str) -> PathBuf {
    paths.agents_dir().join(format!("{name}.key"))
}

fn watermark_path(paths: &StorePaths, name: &str) -> PathBuf {
    paths.agents_dir().join(format!("{name}.seq"))
}

/// Load the key an agent signs with.
pub fn load(paths: &StorePaths, name: &str) -> Result<AgentKey> {
    check_name(name)?;
    let raw = fs::read(key_path(paths, name)).map_err(|_| {
        Error::Refused(format!(
            "there is no agent called {name} on this device. Make one with:  capture agent add {name}"
        ))
    })?;
    let file: AgentFile = minicbor::decode(&raw)
        .map_err(|e| Error::Integrity(format!("agent key {name} unreadable: {e}")))?;
    if file.version != AGENT_FILE_VERSION {
        return Err(Error::Integrity(format!(
            "agent key {name} is version {}, expected {AGENT_FILE_VERSION}",
            file.version
        )));
    }
    Ok(assemble(name, &file))
}

fn assemble(name: &str, file: &AgentFile) -> AgentKey {
    let signing = SigningKey::from_bytes(&file.secret);
    let incarnation = Incarnation(file.incarnation);
    let dev = Dev::derive(&signing.verifying_key().to_bytes(), &incarnation);
    AgentKey {
        name: name.to_string(),
        signing,
        incarnation,
        dev,
    }
}

/// Make a fresh key for `name`, refusing if one is already here.
fn create_key(paths: &StorePaths, name: &str) -> Result<AgentKey> {
    check_name(name)?;
    let path = key_path(paths, name);
    if path.exists() {
        return Err(Error::Usage(format!(
            "there is already an agent called {name}"
        )));
    }
    fs::create_dir_all(paths.agents_dir())?;
    let mut secret = [0u8; 32];
    crate::entropy::fill(&mut secret)?;
    let mut inc = [0u8; 16];
    crate::entropy::fill(&mut inc)?;
    let file = AgentFile {
        version: AGENT_FILE_VERSION,
        secret,
        incarnation: inc,
    };
    let mut b = Vec::new();
    minicbor::encode(&file, &mut b).expect("encoding into Vec is infallible");
    crate::identity::write_secret(&path, &b)?;
    Ok(assemble(name, &file))
}

/// How far an agent has written, as recorded beside its key.
pub(crate) fn watermark(paths: &StorePaths, name: &str) -> u64 {
    fs::read_to_string(watermark_path(paths, name))
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0)
}

pub(crate) fn advance(paths: &StorePaths, name: &str, seq: u64) -> Result<()> {
    if seq > watermark(paths, name) {
        crate::identity::write_plain(&watermark_path(paths, name), seq.to_string().as_bytes())?;
    }
    Ok(())
}

/// Enrol a new agent: a key on this device, and a roster admission signed by
/// this device saying it is an agent called `name`.
///
/// The admission is the device's act, so it is refused while writing as an
/// agent — an agent cannot make another.
pub fn add(paths: &StorePaths, name: &str) -> Result<Agent> {
    check_name(name)?;
    if paths.acting.is_some() {
        return Err(Error::Refused("an agent cannot enrol another agent".into()));
    }
    let mut s = crate::session::Session::open(paths.clone())?;
    let roster = capture_core::roster::build(&s.log.all()?);
    if roster.key_for(&s.identity.dev()).is_none() {
        // The admission would be refused as signed by a non-member, and the
        // key would sit on disk enrolled in nothing.
        return Err(Error::Usage(
            "this device is not in its own roster yet, so it cannot enrol an agent".into(),
        ));
    }
    if roster.agents.values().any(|n| n == name) {
        return Err(Error::Usage(format!(
            "an agent called {name} is already in the roster (perhaps enrolled on another device)"
        )));
    }
    let key = create_key(paths, name)?;
    let made = s.admit_agent(&key);
    if made.is_err() {
        // Nothing points at a key whose admission was never written.
        let _ = fs::remove_file(key_path(paths, name));
    }
    made?;
    Ok(Agent {
        name: name.to_string(),
        pubkey: key.public(),
        retired: false,
        here: true,
    })
}

/// Every agent in the roster, retired ones included.
pub fn list(paths: &StorePaths) -> Result<Vec<Agent>> {
    let log = crate::log::Log::open_read(&paths.events)?;
    let roster = capture_core::roster::build(&log.all()?);
    let mut out: Vec<Agent> = roster
        .agents
        .iter()
        .map(|(k, name)| Agent {
            name: name.clone(),
            pubkey: *k,
            retired: roster.retired.contains_key(k),
            here: load(paths, name).is_ok_and(|a| a.public() == *k),
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

/// Load `name`'s key and check the roster lets it write now.
pub(crate) fn signer(paths: &StorePaths, name: &str, log: &crate::log::Log) -> Result<AgentKey> {
    let key = load(paths, name)?;
    let roster = capture_core::roster::build(&log.all()?);
    if roster.agent(&key.dev()) != Some(name) {
        return Err(Error::Refused(format!(
            "{name}'s key is not enrolled as an agent in this archive"
        )));
    }
    if roster.retired.contains_key(&key.public()) {
        return Err(Error::Refused(format!(
            "the agent {name} has been retired and can no longer write"
        )));
    }
    let head = log
        .heads()?
        .get(&crate::session::dev_hex(&key.dev()))
        .copied()
        .unwrap_or(0);
    let wrote = watermark(paths, name);
    if wrote > head {
        return Err(Error::Refused(format!(
            "this archive holds {head} of {name}'s events and {name} has written {wrote}: \
             the log is older than the agent. Retire it and add a new one rather than \
             reuse positions other devices already hold."
        )));
    }
    Ok(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_safe_file_names() {
        for good in ["claude", "a", "build-bot-2"] {
            assert!(check_name(good).is_ok(), "{good}");
        }
        for bad in ["", "Claude", "../x", "a b", "-x", "a.key", &"x".repeat(33)] {
            assert!(check_name(bad).is_err(), "{bad}");
        }
    }
}
