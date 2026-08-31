//! What a device broadcasts before anyone has authenticated.
//!
//! mDNS is sent in the clear to every host on the network, so this module's
//! whole job is to say enough that a roster member recognises a peer and
//! nothing that anyone else can use. §5.2 names the two leaks a naive
//! advertisement has, and both are cheap to close here and expensive to close
//! once devices are advertising in the wild:
//!
//! - **Correlation.** A stable identifier broadcast on every network you join
//!   links your phone, laptop and desktop to each other at every café, office
//!   and airport — to precisely the guest phone and housemate's laptop the
//!   threat model names.
//! - **Counting.** A plain digest of the version vector changes on every
//!   append, so an observer learns when and how often you capture without ever
//!   completing a handshake: "fourteen notes during that meeting".
//!
//! Both are closed by keying the advertised values with the roster secret and
//! rotating them on a schedule. The rotation is what closes the counting leak:
//! a keyed digest still changes whenever the vector changes, so keying alone
//! would hide the count and leave the timing. Because the value also changes
//! every epoch on its own, a change no longer implies an append.

use capture_core::Dev;

/// How long an advertised value stands before it rotates, in seconds.
///
/// Ten minutes is short enough that append timing is buried in scheduled
/// churn, and long enough that a browse started at random usually sees a peer
/// under the same value it will still be using when the connection opens.
pub const EPOCH: u64 = 600;

/// The advertised bytes, hex-encoded into a TXT record.
///
/// Eight bytes: a passive observer gets nothing from it whatever its length,
/// and the only collision that matters is between the handful of devices in
/// one roster on one network.
pub const ADVERTISED: usize = 8;

/// The mDNS service devices advertise and browse.
pub const SERVICE: &str = "_bsp._tcp.local.";

/// The port a device listens for peers on.
///
/// Fixed rather than ephemeral, and named here rather than in the daemon,
/// because two things need to agree on it: the daemon that listens, and
/// pairing, which records where a peer can be reached before any daemon has
/// been seen. A port that changed on every restart would make every
/// remembered address wrong — on precisely the networks where mDNS is blocked
/// and a remembered address is all there is.
///
/// Overridable for the case of two stores on one machine, which is otherwise
/// impossible to run and is exactly what testing needs.
pub const SYNC_PORT: u16 = 41822;

/// The port to listen on, honouring `CAPTURE_SYNC_PORT`.
pub fn sync_port() -> u16 {
    std::env::var("CAPTURE_SYNC_PORT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(SYNC_PORT)
}

/// Which network this is, as far as the device can tell.
///
/// Opaque on purpose. What identifies a network differs by platform and by
/// what is available — gateway MAC, SSID, subnet — and this module needs only
/// that the answer is stable while attached to one network and different on
/// another. A caller that can offer nothing stable should pass something
/// per-boot rather than a constant: rotating too often costs discovery
/// latency, and never rotating is the leak.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Network(pub [u8; 16]);

impl Network {
    /// Fold whatever the platform can say about the network into an identifier.
    pub fn of(evidence: &[u8]) -> Self {
        let h = blake3::hash(evidence);
        let mut out = [0u8; 16];
        out.copy_from_slice(&h.as_bytes()[..16]);
        Self(out)
    }
}

/// Which rotation period a wall-clock second falls in.
pub fn epoch_at(unix_seconds: u64) -> u64 {
    unix_seconds / EPOCH
}

/// Derive an advertised value, domain-separated by purpose.
///
/// The secret is the key rather than an input: an observer holding neither
/// cannot distinguish these from random, and cannot tell two advertisements
/// by the same device on two networks from two different devices.
fn mac(secret: &[u8; 32], purpose: &str, network: &Network, epoch: u64, input: &[u8]) -> [u8; 8] {
    let mut h = blake3::Hasher::new_keyed(secret);
    h.update(purpose.as_bytes());
    h.update(&network.0);
    h.update(&epoch.to_be_bytes());
    h.update(input);
    let mut out = [0u8; ADVERTISED];
    out.copy_from_slice(&h.finalize().as_bytes()[..ADVERTISED]);
    out
}

/// What this device calls itself on this network, this epoch.
pub fn pseudonym(secret: &[u8; 32], network: &Network, epoch: u64, pubkey: &[u8; 32]) -> [u8; 8] {
    mac(secret, "bsp-pseudonym", network, epoch, pubkey)
}

/// A keyed summary of how far along every origin this device holds is.
///
/// A peer compares it against the same function of the vector it last saw
/// from that device: equal means there is nothing to fetch and the connection
/// can be skipped entirely (§5.2). Unequal means something changed — not what,
/// and not how much.
pub fn digest(secret: &[u8; 32], network: &Network, epoch: u64, heads: &[(Dev, u64)]) -> [u8; 8] {
    let mut sorted: Vec<(Dev, u64)> = heads.to_vec();
    // Two replicas holding the same events must produce the same digest, and
    // nothing guarantees the caller's iteration order.
    sorted.sort_by_key(|(dev, _)| dev.0);

    let mut input = Vec::with_capacity(sorted.len() * 40);
    for (dev, seq) in &sorted {
        input.extend_from_slice(&dev.0);
        input.extend_from_slice(&seq.to_be_bytes());
    }
    mac(secret, "bsp-digest", network, epoch, &input)
}

/// Which member, if any, is advertising under this pseudonym.
///
/// The previous epoch is accepted as well as the current one. A device
/// advertises a value that stands for ten minutes and cannot re-broadcast the
/// instant it turns over, and two devices' clocks differ; without this, every
/// rotation boundary is a window where members stop seeing each other for no
/// reason a user could ever diagnose.
pub fn recognise(
    secret: &[u8; 32],
    network: &Network,
    epoch: u64,
    members: &[[u8; 32]],
    advertised: &[u8; 8],
) -> Option<[u8; 32]> {
    for candidate in members {
        for e in [epoch, epoch.saturating_sub(1)] {
            if &pseudonym(secret, network, e, candidate) == advertised {
                return Some(*candidate);
            }
        }
    }
    None
}

/// How long to wait before dialling a peer again, in seconds.
///
/// For the static fallback: mDNS is blocked on many corporate and guest
/// networks, and a system that only syncs at home is not a system, so a device
/// also retries the addresses it reached a peer at last time. Exponential so a
/// peer that is genuinely off does not get dialled every second all day;
/// jittered because the alternative is every device on the network waking to
/// retry in the same instant, which is the thundering herd that makes a
/// recovering network fail to recover.
pub fn backoff(attempt: u32, jitter: u64) -> u64 {
    const FIRST: u64 = 2;
    const CEILING: u64 = 300;

    let base = FIRST.saturating_mul(1u64 << attempt.min(8)).min(CEILING);
    // Up to a quarter of the interval, added rather than subtracted: the point
    // is to spread retries out, not to make the first one arrive sooner.
    base + (jitter % (base / 4 + 1))
}

/// One advertisement seen on the network.
///
/// Deliberately not an mDNS type. Everything that decides what to do with a
/// sighting is a pure function of this, so the deciding is testable without a
/// network — and mDNS is a source of these rather than something the logic
/// knows about, which is what makes the static fallback able to produce them
/// too.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Sighting {
    /// What the device is calling itself this epoch.
    pub pseudonym: [u8; 8],
    /// What it says its version vector amounts to.
    pub digest: [u8; 8],
    /// Where to reach it.
    pub addr: String,
}

/// The TXT key carrying the pseudonym.
pub const TXT_NAME: &str = "n";
/// The TXT key carrying the digest.
pub const TXT_DIGEST: &str = "d";

/// A member seen on the network, and what it was advertising.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Seen {
    pub peer: [u8; 32],
    pub digest: [u8; 8],
    pub addr: String,
}

/// Decide whether a sighting is a peer worth talking to.
///
/// `None` for anything this device cannot recognise, which is the common case:
/// a printer, a media player, another household's laptop. Nothing is dialled
/// on the strength of being on the same network — proximity on the LAN is not
/// membership (§5.3), and the handshake would refuse it anyway. This just
/// avoids opening the connection to find out.
pub fn consider(
    secret: &[u8; 32],
    network: &Network,
    epoch: u64,
    members: &[[u8; 32]],
    sighting: &Sighting,
) -> Option<Seen> {
    let peer = recognise(secret, network, epoch, members, &sighting.pseudonym)?;
    Some(Seen {
        peer,
        digest: sighting.digest,
        addr: sighting.addr.clone(),
    })
}

/// Render an advertised value for a TXT record.
pub fn encode_value(v: &[u8; 8]) -> String {
    let mut s = String::with_capacity(16);
    for b in v {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

/// Read an advertised value back.
///
/// Returns `None` for anything that is not exactly the expected shape. Every
/// other service on the network puts whatever it likes in its TXT records, so
/// this is the ordinary case rather than an error worth reporting.
pub fn decode_value(s: &str) -> Option<[u8; 8]> {
    if s.len() != ADVERTISED * 2 {
        return None;
    }
    let mut out = [0u8; ADVERTISED];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(s.get(i * 2..i * 2 + 2)?, 16).ok()?;
    }
    Some(out)
}
