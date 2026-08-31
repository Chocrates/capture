//! The mDNS side of discovery: advertising, and turning what is on the
//! network into [`Sighting`]s.
//!
//! Everything that *decides* anything lives in [`crate::discovery`] and is a
//! pure function. This module only moves bytes between that and the network,
//! which is why it is the one part of discovery without tests: there is
//! nothing here to get wrong that a test without a network could catch, and a
//! test with one would be the skipped test §5 warns about.

use std::collections::HashSet;
use std::net::IpAddr;

use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};

use crate::discovery::{
    ADVERTISED, SERVICE, Sighting, TXT_DIGEST, TXT_NAME, decode_value, encode_value,
};

/// What went wrong reaching the network.
#[derive(Debug)]
pub enum DiscoveryError {
    /// The mDNS responder could not be started, or the advertisement refused.
    ///
    /// Not fatal, and deliberately not reported as such: mDNS is blocked on
    /// many corporate and guest networks, which is the situation the static
    /// fallback exists for. A device that treated this as an error would
    /// refuse to sync on exactly the networks where it most needs to.
    Unavailable(String),
}

impl std::fmt::Display for DiscoveryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unavailable(why) => write!(f, "mDNS unavailable: {why}"),
        }
    }
}

impl std::error::Error for DiscoveryError {}

/// An advertisement on the local network, and a browse of everyone else's.
pub struct Beacon {
    daemon: ServiceDaemon,
    registered: Option<String>,
}

impl Beacon {
    pub fn new() -> Result<Self, DiscoveryError> {
        let daemon =
            ServiceDaemon::new().map_err(|e| DiscoveryError::Unavailable(e.to_string()))?;
        Ok(Self {
            daemon,
            registered: None,
        })
    }

    /// Say the two things §5.2 permits: a name that means nothing to an
    /// outsider, and a digest that tells a member whether to bother.
    ///
    /// Re-registering under a new name is how rotation happens. The old
    /// instance is withdrawn first, so a member browsing does not see the same
    /// device twice under two names and count it as two peers.
    pub fn advertise(
        &mut self,
        pseudonym: &[u8; ADVERTISED],
        digest: &[u8; ADVERTISED],
        port: u16,
        addresses: &[IpAddr],
    ) -> Result<(), DiscoveryError> {
        let name = encode_value(pseudonym);
        let host = format!("{name}.local.");

        let info = ServiceInfo::new(
            SERVICE,
            &name,
            &host,
            addresses,
            port,
            &[
                (TXT_NAME, name.as_str()),
                (TXT_DIGEST, &encode_value(digest)),
            ][..],
        )
        .map_err(|e| DiscoveryError::Unavailable(e.to_string()))?;

        if let Some(old) = self.registered.take() {
            // Ignored: the withdrawal is best-effort tidiness, and failing it
            // is not a reason to stop advertising.
            let _ = self.daemon.unregister(&old);
        }
        self.registered = Some(info.get_fullname().to_string());

        self.daemon
            .register(info)
            .map_err(|e| DiscoveryError::Unavailable(e.to_string()))
    }

    /// Collect what is visible right now.
    ///
    /// Non-blocking: whatever has resolved by the time this is called. mDNS is
    /// a continuous conversation rather than a query with an answer, so a
    /// caller polls this on the same schedule it does anything else.
    pub fn sightings(&self) -> Result<Vec<Sighting>, DiscoveryError> {
        let rx = self
            .daemon
            .browse(SERVICE)
            .map_err(|e| DiscoveryError::Unavailable(e.to_string()))?;

        let mut out = Vec::new();
        let mut seen = HashSet::new();

        while let Ok(event) = rx.try_recv() {
            let ServiceEvent::ServiceResolved(svc) = event else {
                continue;
            };
            let (Some(name), Some(digest)) = (
                svc.get_property_val_str(TXT_NAME).and_then(decode_value),
                svc.get_property_val_str(TXT_DIGEST).and_then(decode_value),
            ) else {
                // Some other service, or a version of this one that predates
                // the fields. Nothing to say about it.
                continue;
            };
            let Some(addr) = svc.get_addresses().iter().next() else {
                continue;
            };
            if !seen.insert(name) {
                continue;
            }
            out.push(Sighting {
                pseudonym: name,
                digest,
                addr: format!("{}:{}", addr.to_ip_addr(), svc.get_port()),
            });
        }

        Ok(out)
    }
}

impl Drop for Beacon {
    fn drop(&mut self) {
        // Withdraw rather than letting the record time out. A peer that goes
        // on advertising after it has stopped listening is a peer everyone
        // else spends their backoff on.
        if let Some(name) = &self.registered {
            let _ = self.daemon.unregister(name);
        }
        let _ = self.daemon.shutdown();
    }
}
