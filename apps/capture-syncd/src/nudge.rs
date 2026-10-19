//! Two reasons to stop waiting for the timer.
//!
//! Anti-entropy runs on a timer, and a timer alone means the two worst moments
//! for staleness are the two it handles worst: the laptop that has just been
//! opened, and the machine that has just joined a network where its peers
//! actually are. Both are moments the system is *told* about, and waiting
//! thirty seconds to discover them is a choice rather than a constraint.
//!
//! # Why the system bus, and why a thread each
//!
//! Both signals come from daemons everyone already runs — logind for sleep,
//! NetworkManager for connectivity — so there is nothing to install and
//! nothing to poll. Each subscription gets its own thread that sits in
//! `zbus::block_on` for as long as it listens: one subscription does not need
//! a scheduler, and zbus drives the connection on the thread that waits.
//!
//! # Best effort, always
//!
//! Neither signal is required for correctness. A device that never hears
//! either still syncs on its timer, so every failure here — no system bus, no
//! NetworkManager, a match rule the broker rejects — is logged once and
//! dropped. A daemon that refused to start because logind was absent would be
//! a daemon that refuses to start in a container.

use std::sync::mpsc::Sender;

use futures_lite::StreamExt;
use zbus::{Connection, Proxy};

/// What woke us.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Nudge {
    /// The machine has come back from sleep.
    ///
    /// Peers have moved on while it was away, and it holds captures they have
    /// not seen.
    Woke,
    /// The network changed under it.
    ///
    /// Everything discovery knows is now wrong: remembered addresses belong to
    /// the old network, and the advertised pseudonyms were derived against it,
    /// so peers here would not be recognised even if they were visible.
    Network,
}

/// Watch for both, on their own threads.
///
/// Returns immediately. Failures are reported once and then silent: the timer
/// is the mechanism and these are an improvement on it.
fn watch(tx: Sender<Nudge>) {
    let woke = tx.clone();
    std::thread::spawn(move || {
        if let Err(e) = sleep_signals(&woke) {
            tracing::info!(%e, "no wake notifications; syncing on the timer alone");
        }
    });

    std::thread::spawn(move || {
        if let Err(e) = network_signals(&tx) {
            tracing::info!(%e, "no network-change notifications; syncing on the timer alone");
        }
    });
}

/// `PrepareForSleep(false)` means the machine has resumed.
///
/// The same signal fires with `true` just *before* suspending, which is not
/// interesting: there is no point starting a sync a moment before the network
/// goes away.
fn sleep_signals(tx: &Sender<Nudge>) -> zbus::Result<()> {
    zbus::block_on(async {
        let conn = Connection::system().await?;
        let proxy = Proxy::new(
            &conn,
            "org.freedesktop.login1",
            "/org/freedesktop/login1",
            "org.freedesktop.login1.Manager",
        )
        .await?;

        let mut signals = proxy.receive_signal("PrepareForSleep").await?;
        while let Some(signal) = signals.next().await {
            let going_to_sleep: bool = signal.body().deserialize()?;
            if !going_to_sleep && tx.send(Nudge::Woke).is_err() {
                return Ok(()); // the daemon is shutting down
            }
        }
        Ok(())
    })
}

/// NetworkManager's overall state, which changes on every connect and
/// disconnect.
///
/// Deliberately not filtered to "connected": a disconnect is worth knowing
/// about too, because it is what makes the remembered addresses stale. The
/// round that follows finds nothing and costs nothing.
fn network_signals(tx: &Sender<Nudge>) -> zbus::Result<()> {
    zbus::block_on(async {
        let conn = Connection::system().await?;
        let proxy = Proxy::new(
            &conn,
            "org.freedesktop.NetworkManager",
            "/org/freedesktop/NetworkManager",
            "org.freedesktop.NetworkManager",
        )
        .await?;

        let mut signals = proxy.receive_signal("StateChanged").await?;
        while signals.next().await.is_some() {
            if tx.send(Nudge::Network).is_err() {
                return Ok(());
            }
        }
        Ok(())
    })
}

/// Start watching, delivering into the daemon's own channel.
///
/// A small adapter thread rather than making the signal watchers know what an
/// `Arrival` is: the two D-Bus subscriptions have nothing to do with sockets,
/// and keeping them ignorant of the daemon's loop is what lets them be tested
/// and reasoned about on their own.
pub fn watch_into(into: Sender<crate::Arrival>) {
    let (tx, rx) = std::sync::mpsc::channel();
    watch(tx);
    std::thread::spawn(move || {
        for n in rx {
            if into.send(crate::Arrival::Nudged(n)).is_err() {
                return;
            }
        }
    });
}
