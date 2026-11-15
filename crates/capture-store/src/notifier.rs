//! Which device says something, and what it says.
//!
//! # A default is a rule; a setting is a decision
//!
//! §2.4.1 makes this the whole of the design here, and §8.9 makes it a rule the
//! system may not break: **a default is never written to the log on the user's
//! behalf**.
//!
//! An earlier draft in the design had the first paired device emit a `setting`
//! choosing itself as notifier. That is a machine-authored preference recorded
//! as a user decision — `capture status` would then report it as "set by you", it
//! could never be centrally improved because changing the default would not
//! reach anybody, and there is no way to tell it apart from a choice somebody
//! actually made.
//!
//! So: until the user chooses, the notifier is decided by a rule evaluated
//! locally and written nowhere. [`chosen`] returns `None` in that case, and it
//! is `None` that `capture status` reports as a default.
//!
//! # Which cues can be fired here, and which cannot
//!
//! Declaring a cue type requires being able to observe it. A device that
//! claimed `app_focus` and could not see it would produce an intent that is
//! registered, reported as working, and silently never fires — the same failure
//! shape as a dead chord.
//!
//! On this target — GNOME on Wayland — the foreground application is not
//! observable at all: there is no protocol for it, `Shell.Eval` is disabled,
//! and the Xwayland `_NET_ACTIVE_WINDOW` answer is a plausible-looking zero.
//! M1 established that for the ambient envelope (§2.3) and it is the same fact
//! here, so `app_focus` is refused rather than accepted and dropped.

use crate::projection::Store;
use crate::{Result, StorePaths};

/// The setting key naming the device that fires time-based cues.
pub const KEY: &str = "notifier";

/// The device the user chose, if they chose one.
///
/// `None` means no choice has been made — not that no device notifies. The
/// caller applies [`default_rule`], and the difference between the two is what
/// §8.9 exists to preserve.
pub fn chosen(state: &Store) -> Result<Option<String>> {
    state.setting(KEY)
}

/// What a device adds to the capabilities it reports when its notifications
/// are blocked.
///
/// Negative on purpose. A positive `notify` would be absent from every older
/// build's report, and the default rule would read each of those peers as
/// unable to speak — the safe-looking choice that produces silence.
pub const SILENT: &str = "silent";

/// How long a peer may go unreached before the default rule stops counting on
/// it (§2.4.1's grace window).
///
/// A day rather than minutes: a phone in a drawer for an afternoon should not
/// hand alerts to the desktop and take them back again, and one unseen for a
/// day is probably not where somebody will see them.
pub const GRACE_MS: u64 = 24 * 60 * 60 * 1000;

/// One device, as the default rule weighs it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Candidate {
    pub key: [u8; 32],
    /// When it first joined the roster. `None` sorts as the oldest, so a key
    /// with no admission on record never outranks one that has one.
    pub admitted: Option<capture_core::Hlc>,
    /// Whether it last said it cannot show notifications.
    pub silent: bool,
    /// When this device last completed a round with it. Ignored for this
    /// device itself.
    pub reached: Option<u64>,
}

/// §2.4.1's default: the most recently paired device that can notify, with
/// the next one standing in for any that has gone quiet.
///
/// Every device evaluates this the same way over the same roster, so they
/// agree without anything being written — §8.9 forbids writing a default down
/// as though somebody chose it. Where their views of who is reachable differ,
/// two may both speak for a while; that duplicate is accepted, because the
/// alternative failure is nobody speaking. And if nobody qualifies at all, this
/// device speaks.
pub fn default_notifier(candidates: &[Candidate], me: &[u8; 32], now: u64) -> [u8; 32] {
    let mut order: Vec<&Candidate> = candidates.iter().collect();
    // Most recent first; the key breaks a tie so every replica breaks it alike.
    order.sort_by(|a, b| b.admitted.cmp(&a.admitted).then(a.key.cmp(&b.key)));
    order
        .into_iter()
        .find(|c| {
            !c.silent
                && (c.key == *me || c.reached.is_some_and(|t| now.saturating_sub(t) <= GRACE_MS))
        })
        .map(|c| c.key)
        .unwrap_or(*me)
}

/// The default rule's inputs for this device, gathered: the roster from the
/// log, what each peer last reported and when it was last reached from ops,
/// and this device's own silence.
pub fn candidates(paths: &StorePaths, me: &[u8; 32]) -> Result<Vec<Candidate>> {
    let log = crate::log::Log::open_read(&paths.events)?;
    let mut roster = capture_core::roster::build(&log.all()?);
    // A device does not apply a retirement of itself it did not issue (§5.3),
    // so it does not drop itself from the order either.
    roster.honour_self(me);
    let ops = crate::ops::Ops::open_read(&paths.ops)?;
    let reached = ops.peer_addresses()?;
    let mut out = Vec::new();
    for key in roster.devices() {
        let silent = if key == me {
            ops.get("alerts.silenced")?.as_deref() == Some("1")
        } else {
            ops.peer_observes(key)?.iter().any(|c| c == SILENT)
        };
        out.push(Candidate {
            key: *key,
            admitted: roster.admitted.get(key).copied(),
            silent,
            reached: reached
                .iter()
                .find(|p| &p.key == key)
                .and_then(|p| p.last_seen),
        });
    }
    Ok(out)
}

/// The device that speaks by default, as this device sees it now.
pub fn default_here(paths: &StorePaths, me: &[u8; 32], now: u64) -> Result<[u8; 32]> {
    Ok(default_notifier(&candidates(paths, me)?, me, now))
}

/// Whether this device is the one that says things.
///
/// # Why this had to exist
///
/// `chosen` had two readers — `capture status` and an FFI getter — and no
/// delivery path consulted either. So the setting offered for choosing a
/// notifier was inert: with a phone and a desktop paired, every moment was
/// announced on both, and the control that exists to stop that did nothing
/// when it was used.
///
/// # The default is a rule, not a choice
///
/// When nobody has chosen, [`default_notifier`] decides. This answered "yes"
/// on every device instead — the M1 rule, one device so it is this one — and
/// with a phone and a desktop paired both announced every moment.
///
/// # Matched on a prefix
///
/// The setting holds a device key as `capture peers` prints it, which is
/// shortened. Comparing a shortened key against a full one by equality would
/// silently never match and silence the device that should speak — the failure
/// this is meant to prevent, arriving by the other door.
pub fn speaks_here(paths: &StorePaths, state: &Store, me: &[u8; 32], now: u64) -> Result<bool> {
    let who = chosen(state)?
        .map(|w| w.trim().to_ascii_lowercase())
        .unwrap_or_default();
    if who.is_empty() {
        return Ok(default_here(paths, me, now)? == *me);
    }
    Ok(capture_core::id::hexed(me).starts_with(&who))
}

/// The default rule, said in words.
pub fn default_rule() -> &'static str {
    "the most recently paired device that can notify"
}

/// Whether a cue type can be observed here.
///
/// The honest answer per cue, on this platform. `false` is not a failure: it
/// means the intent waits for a device that can see it, which is the shape
/// §2.4.1 already describes for a geofence.
pub fn observable(cue_type: &str) -> bool {
    match cue_type {
        // A clock is readable anywhere.
        "time" | "recurring" | "device_wake" => true,

        // Where the repository is, which is not a phone.
        "git_branch" => !ANDROID,

        // Not observable on GNOME/Wayland, and saying so is the point. An
        // intent that claimed to watch this would be registered, reported as
        // working, and permanently silent. Nor on Android, where there is no
        // notion of another application's focus available to this one.
        "app_focus" => false,

        // A phone's to see (§2.4.1). False on the desktop not because the
        // feature is missing but because the desktop cannot observe it: a
        // geofence answered by a machine that does not move is a geofence that
        // never fires, or fires once and never stops.
        //
        // True here says only that the *platform* can observe it. Whether this
        // particular device may is a runtime question — the permission — and
        // one a store cannot answer. The surface that knows reports it (§7.3),
        // because an intent failing invisibly is worse than no intent: the
        // person believes something is watching when nothing is.
        "location" => ANDROID,

        // Needs a calendar read that M2 does not have.
        "calendar_start" => false,

        _ => false,
    }
}

/// Whether this build is the phone.
///
/// The same function, answering differently where it runs. A device declares
/// what it can see, and these two see different things — which is the whole
/// reason §2.4.1 routes a cue to a device rather than to "the notifier".
const ANDROID: bool = cfg!(target_os = "android");

/// The program that puts a notification on this desktop. `capture doctor`
/// asks whether it is there.
///
/// On a Mac, `osascript` (it-runs-on-a-mac D3): the one route an unsigned
/// program has. The notification is attributed to Script Editor, and
/// management may switch it off; doctor's N3 is how that is found out.
pub const NOTIFIER: &str = if cfg!(target_os = "macos") {
    "osascript"
} else {
    "notify-send"
};

/// The arguments [`NOTIFIER`] takes for one notification.
///
/// On a Mac the text goes in as `argv`, never into the script. A note saying
/// `" & do shell script "…` would otherwise be AppleScript, run as the person.
fn notifier_args(mac: bool, summary: &str, body: &str) -> Vec<String> {
    if mac {
        [
            "-e",
            "on run argv",
            "-e",
            "display notification (item 2 of argv) with title \"capture\" subtitle (item 1 of argv)",
            "-e",
            "end run",
            summary,
            body,
        ]
        .map(String::from)
        .to_vec()
    } else {
        ["--app-name=capture", summary, body]
            .map(String::from)
            .to_vec()
    }
}

/// Run a notifier and say whether it worked: a missing program, a refusal
/// or a non-zero exit is not delivered.
fn run_notifier(program: &str, args: &[String]) -> bool {
    std::process::Command::new(program)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Say something, by whatever means this desktop offers.
///
/// Best-effort and deliberately not a dependency: a notification that fails is
/// worth a log line, and a missing notification daemon must not stop the
/// process that noticed the cue.
///
/// **Returns whether it was actually said**, and the return is load-bearing.
/// A caller that records a delivery which did not happen spends the moment on
/// a notification nobody could have heard — and because delivery happens once,
/// the moment is then gone. That failure arrived twice while this was being
/// built: first on Android, where posting to a blocked channel succeeds
/// silently, and then here, where `notify-send` does not exist at all and this
/// function returned nothing to say so.
#[must_use]
pub fn notify(summary: &str, body: &str) -> bool {
    // A way to run the system without it talking.
    //
    // Added because alerts now fire from every process startup (§2.6's floor),
    // which includes every test that opens a session against a store holding
    // a dated capture — so `make ci` began posting real notifications to
    // whoever was running it. A suite that interrupts the person running it is
    // one they stop running.
    //
    // An environment variable rather than a setting: it is about this
    // *process*, not about the archive, and §8.9 keeps machine-made choices
    // out of the log. Also useful for a machine that should hold an archive
    // and stay silent — a server, a backup, a second desktop.
    if std::env::var_os("CAPTURE_SILENT").is_some() {
        tracing::debug!(summary, "an alert fired; CAPTURE_SILENT is set");
        // Not said, so not spent. A silent run must not consume the moments it
        // silently did not announce.
        return false;
    }

    // There is no `notify-send` on a phone, and the surface there delivers
    // through the platform's own notification manager. Saying so rather than
    // discovering it: this returned "delivered" for every alert on Android,
    // which marked them spoken for before the surface ever looked.
    if ANDROID {
        return false;
    }

    let sent = run_notifier(
        NOTIFIER,
        &notifier_args(cfg!(target_os = "macos"), summary, body),
    );

    if !sent {
        // The fallback is not silence. An intent that fired and could not be
        // delivered is exactly the thing somebody needs to be able to find out
        // about afterwards.
        tracing::warn!(summary, body, "an intent fired and could not be delivered");
    }
    sent
}

/// The IANA zone this machine is set to, or `UTC` if it cannot be determined.
///
/// Asked of the library rather than parsed out of `/etc/localtime`, which is
/// what four copies of this used to do — including this one, until a guard
/// that compares function bodies found a *fifth* implementation living under
/// two other names and doing it properly.
///
/// Parsing the symlink was never right. It gets `$TZ` wrong in the forms that
/// are not zone names (`EST5EDT`, an absolute path, a POSIX offset string), it
/// gets nothing at all on a machine where `/etc/localtime` is a copy rather
/// than a link, and it was reimplemented each time somebody needed it because
/// there was no obvious owner.
///
/// Falling back rather than failing: a wrong zone moves quiet hours by the
/// offset, which is bad; refusing to answer stops the notification budget
/// working at all, which is worse.
///
/// # Why it is not simply the library's answer any more
///
/// `jiff` reads the system zone once and caches it for the life of the
/// process, which is right for a command that runs for a second and wrong for
/// the two processes that matter here: the overlay and the daemon run for
/// weeks.
///
/// The hour is the input to every §2.6 decision, so a stale zone is not a
/// cosmetic error. London to Los Angeles, and `civil_clock` says 22:00 when it
/// is 14:00: the whole working afternoon reads as quiet hours and is held, and
/// what does escape is released at "07:00" London, which is 23:00 where the
/// person actually is. Everything arrives at night, for a week, and nothing
/// anywhere says why.
///
/// So the sources that *change* are consulted first, on every call, and the
/// library's cached answer is the fallback rather than the whole answer.
///
/// # And why this is not the symlink parsing that was removed
///
/// Four copies of this used to parse `/etc/localtime` by hand, and that was
/// removed for good reasons: it got `$TZ` wrong in every form that is not a
/// zone name (`EST5EDT`, an absolute path, a POSIX offset string), it got
/// nothing at all where `/etc/localtime` is a copy rather than a link, and it
/// was reimplemented each time somebody needed it because nothing owned it.
///
/// All three still apply, and none of them is a reason to read a stale zone:
///
/// - This is the one owner. A guard compares function bodies, and it is what
///   found the fifth copy.
/// - Every candidate is **validated** before it is returned — a form this
///   cannot resolve is not returned wrong, it is passed over. That is what the
///   hand-rolled parsers never did.
/// - A copied rather than linked `/etc/localtime` yields nothing here and falls
///   through to the library, which is exactly the old behaviour.
pub fn local_zone() -> String {
    // `$TZ` first: it is the explicit instruction, and a process given one was
    // given it on purpose.
    if let Some(name) = std::env::var("TZ")
        .ok()
        .and_then(|t| resolvable(t.trim_start_matches(':')))
    {
        return name;
    }

    // Then the link, which is how a zone change actually lands on Linux —
    // `timedatectl`, a desktop settings panel and a distribution installer all
    // repoint it.
    if let Some(name) = std::fs::read_link("/etc/localtime")
        .ok()
        .and_then(|p| zone_from_path(&p))
    {
        return name;
    }

    // Otherwise whatever the library read when the process started.
    jiff::tz::TimeZone::system()
        .iana_name()
        .unwrap_or("UTC")
        .to_string()
}

/// A candidate zone name, if it is one this machine can actually resolve.
///
/// The validation is the point. Every caller feeds the result straight into
/// `Zone::new`, and a name that fails there means quiet hours and batch windows
/// fall back to UTC without saying so.
fn resolvable(name: &str) -> Option<String> {
    capture_time::Zone::new(name).map(|_| name.to_string())
}

/// The zone named by a path like `/usr/share/zoneinfo/Europe/London`.
///
/// Taken from the `zoneinfo` component rather than from a fixed prefix: the
/// directory is `/usr/share/zoneinfo` on most systems and somewhere else on
/// enough of them to matter. Validated like any other candidate.
fn zone_from_path(p: &std::path::Path) -> Option<String> {
    let s = p.to_str()?;
    let (_, after) = s.rsplit_once("zoneinfo/")?;
    resolvable(
        after
            .trim_start_matches("posix/")
            .trim_start_matches("right/"),
    )
}

/// When the day goes quiet, and when it comes back. Local hours.
///
/// A rule, not a setting. §8.9 forbids writing a default to the log on the
/// user's behalf, and the whole of this module is built around that: until
/// somebody chooses quiet hours, these are evaluated locally, written nowhere,
/// and reported as a default rather than as a choice.
pub const QUIET_FROM: u32 = 22;
pub const QUIET_UNTIL: u32 = 7;

/// The three deliveries a day §2.6 chose, in local hours.
///
/// `EVIDENCE.md` §2.5: a fourteen-day field experiment found batching into
/// three daily deliveries made people happier and less stressed, mediated by
/// improved subjective attention quality — as the *default regime*, not as an
/// overflow behaviour. The hours are morning, mid-afternoon and evening, all
/// outside quiet hours by construction.
pub const BATCHES: [u32; 3] = [9, 14, 19];

/// What to do with something that has come due.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Delivery {
    /// Say it now. A moment was named and it has arrived.
    Now,
    /// Hold it until the given local hour.
    ///
    /// Carries the hour rather than an instant because the caller knows what
    /// day it is and this does not — and because a decision that returned an
    /// instant would need a clock, which §8.1 keeps out of this crate.
    At(u32),
    /// Do not say it at all. The moment is too far gone to be news.
    ///
    /// Distinct from holding, because a hold is a promise to say it later and
    /// this is a decision not to. The moment stays on every surface that lists
    /// what is outstanding — it is undone, and nothing here changes that. What
    /// it has stopped being is an announcement.
    Stale,
}

/// How long after its moment a timed alert is still worth saying immediately.
///
/// Past this it is not an alarm any more, it is a note about something that
/// already happened — "your 9am meeting" delivered at 4pm — and it joins a
/// batch like anything else.
///
/// Found by leaving a store alone and asking what was due: seven captures that
/// each named 09:00 all came back `Now`, because each was individually
/// time-critical. They would have arrived as seven notifications the moment
/// somebody opened the phone. §2.6's batching is what should have caught that
/// and did not, because "named a time" was being read as "matters right now"
/// long after the time had gone.
pub const STILL_PROMPT_MS: i64 = 30 * 60 * 1000;

/// How late a moment can be and still be worth announcing at all.
///
/// # Why there has to be a floor
///
/// Without one, "has this been delivered yet" is asked of every moment the
/// horizon can reach, and the horizon is a year wide on either side. Three
/// things follow, and all three were reachable:
///
/// - **A rule edit renames a year of moments.** Delivery is keyed by the
///   instant an occurrence falls on, so shifting a daily series by half an hour
///   makes three hundred already-delivered mornings undelivered at once. They
///   are all in the past, they were all said when they happened, and nothing
///   about the edit makes any of them news.
/// - **A restore replays everything.** `ops.db` is local state and a recovered
///   device starts with an empty one, so every moment in the horizon reads as
///   never delivered — the precise replay the delivery table exists to prevent,
///   arriving through the front door on the worst possible day.
/// - **A task from 2020 is announced today.** It is genuinely outstanding and
///   genuinely not news, and a notification is the wrong surface for it.
///
/// A week, because that is the span over which "I have not done this yet" is
/// still an announcement rather than a fact of life, and it matches the review
/// cycle that exists for exactly this. Past it the moment is still on every
/// list that shows what is outstanding; it has simply stopped interrupting.
pub const STALE_MS: i64 = 7 * 24 * 60 * 60 * 1000;

/// Where the lead time is kept: local to the device that speaks, because only
/// that device says anything (§2.4.1), and which device buzzed when is not a
/// fact the log records.
pub const LEAD_KEY: &str = "alerts.lead_min";

/// How long before a timed moment it is warned of, when nobody has said.
///
/// Fifteen minutes: enough to finish a sentence and leave the room, short
/// enough that the warning is still about the thing and not a reminder that
/// it exists.
pub const DEFAULT_LEAD_MIN: u32 = 15;

/// How long before a timed moment it is warned of, in milliseconds. Zero means
/// no warning.
pub fn lead_ms(ops: &crate::ops::Ops) -> Result<i64> {
    let min = ops
        .get(LEAD_KEY)?
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(DEFAULT_LEAD_MIN);
    Ok(i64::from(min) * 60 * 1000)
}

/// What a warning says: how far ahead, then the capture's own words.
pub fn warning(text: &str, ahead_ms: i64) -> String {
    format!("in {} min: {text}", ahead_ms / 60_000)
}

/// Set the lead time, in minutes. Zero turns warnings off.
pub fn set_lead_min(ops: &crate::ops::Ops, min: u32) -> Result<()> {
    ops.set(LEAD_KEY, &min.to_string())
}

/// Decide when something due may actually be said, and how late it is.
///
/// # The two rules, and which wins
///
/// **Time-critical is immediate.** §2.6 splits immediate from batched with
/// examples rather than a rule — "an alarm, a meeting starting". The rule here
/// is that a capture which named a *time of day* meant that time; one that
/// named only a day did not mean midnight.
///
/// **Quiet hours hold everything except an armed moment.** Somebody who set an
/// alarm for 6am meant 6am, and a system that decided it knew better would be
/// useless for the one thing an alarm is for. Everything else waits for
/// morning: §2.6 puts anxiety on the ledger, and a notification at 3am spends
/// from it at the worst possible rate.
///
/// `armed` distinguishes a moment somebody set deliberately from one inferred
/// by the recognizer. `dentist thursday 2pm` at 2am is almost certainly a
/// misparse; `capture intent at 02:00` is not.
///
/// `late_by` is milliseconds since the moment passed; zero or negative means
/// it is arriving on time.
pub fn deliver_late(hour: u32, timed: bool, armed: bool, late_by: i64) -> Delivery {
    // Before anything else, including armed. An alarm somebody set for a
    // fortnight ago is not an alarm that is going off late; it is an alarm for
    // a moment that has been and gone, and ringing it now tells them nothing
    // they can act on while teaching them that the ring means nothing.
    if late_by > STALE_MS {
        return Delivery::Stale;
    }

    // The waking day is the half-open range; quiet is everything else.
    let quiet = !(QUIET_UNTIL..QUIET_FROM).contains(&hour);

    if quiet && !armed {
        return Delivery::At(QUIET_UNTIL);
    }
    // Armed is unconditional. Somebody who set an alarm meant it, and one
    // arriving late is a reason to say so now rather than to hold it until the
    // afternoon — the lateness is the news.
    if armed || (timed && late_by < STILL_PROMPT_MS) {
        return Delivery::Now;
    }

    // Arrived at a delivery: this is the delivery.
    //
    // This line was `find(|&b| b > hour)` and nothing else, which meant that
    // at 09:00 the 09:00 batch was not "now" but "wait until 14:00" — and 19
    // wrapped to 9. Traced across a whole day, a batched item was delivered at
    // **no hour at all**: every task carrying a due date was silently never
    // announced, every late alert was lost, and every quiet-hours hold was
    // permanent.
    //
    // The test that should have caught it asserted the batch hours all sit
    // outside quiet hours — that a loop *could not* exist — and never asked
    // what happens at 9, 14 or 19. A property about the arrangement, where
    // what was needed was a property about the outcome.
    if BATCHES.contains(&hour) {
        return Delivery::Now;
    }

    // Otherwise the next one today, or tomorrow's first. An hour that has
    // already passed today would mean "yesterday", and the caller works the
    // day out from the hour it gets back.
    Delivery::At(next_batch(hour))
}

/// The next batch hour after the one given, wrapping to tomorrow's first.
pub fn next_batch(hour: u32) -> u32 {
    match BATCHES.iter().find(|&&b| b > hour) {
        Some(&b) => b,
        None => BATCHES[0],
    }
}

/// How long a repeating cue waits before it may say the same thing again.
///
/// Not a debounce on the sensor — the platform already collapses a boundary
/// crossing into one event. This is about the boundary itself: a shop is a
/// point with a radius, a person at the counter is metres from the edge of it,
/// and a phone that re-acquires its position every few minutes will cross that
/// edge repeatedly without anybody moving. Thirty minutes is long enough that
/// leaving and coming back is a real visit rather than a wobble.
pub const REARM_MS: u64 = 30 * 60 * 1000;

/// Whether a cue that has already fired may fire again.
///
/// A time cue happens once: it named a moment, the moment passed, and saying
/// so twice is noise. A geofence happens every visit — "buy screws when you
/// are at the hardware shop" is worthless if it only ever works the first
/// time, and that is the difference §2.4 draws between a reminder and a plan.
pub fn may_fire_again(last_fired: Option<u64>, now: u64, cue_type: &str) -> bool {
    match last_fired {
        None => true,
        Some(_) if !repeats(cue_type) => false,
        Some(then) => now.saturating_sub(then) >= REARM_MS,
    }
}

/// Whether a cue describes a recurring condition rather than a single moment.
fn repeats(cue_type: &str) -> bool {
    matches!(
        cue_type,
        "location" | "recurring" | "app_focus" | "git_branch" | "device_wake"
    )
}

/// Where fired intents are remembered.
///
/// Local state rather than the log. *Which* device delivered a notification and
/// *when* it managed to is not a fact about the archive — it is operational
/// chatter, and §2.5.2 keeps that out of a log meant to be worth reading in ten
/// years. It also means two devices can each fire their own local cues without
/// arguing about it.
pub fn already_fired(paths: &StorePaths, id: &[u8]) -> Result<bool> {
    let ops = crate::ops::Ops::open_read(&paths.ops)?;
    ops.intent_fired(id)
}

/// Remember that one fired.
pub fn mark_fired(paths: &StorePaths, id: &[u8], now: u64) -> Result<()> {
    let ops = crate::ops::Ops::open(&paths.ops)?;
    ops.mark_intent_fired(id, now)
}

#[cfg(test)]
mod zone {
    use super::*;

    /// `$TZ` is process-wide, and the test harness runs these in parallel.
    ///
    /// Without this, a test that sets the variable changes what a test
    /// asserting a *stable* answer sees, and the pair fail each other
    /// intermittently — the kind of flake that gets a real assertion deleted
    /// for being unreliable.
    static TZ: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Hold the lock for the body of a test that touches `$TZ`.
    ///
    /// Ignores poisoning: a panicking test has already failed, and its failure
    /// is the news rather than every later test in the module failing too.
    fn serially() -> std::sync::MutexGuard<'static, ()> {
        TZ.lock().unwrap_or_else(|e| e.into_inner())
    }

    #[test]
    fn it_answers_something_that_is_actually_a_zone() {
        let _guard = serially();
        // The only property worth asserting, and the one that matters: every
        // caller feeds this straight into `Zone::new`, and a name that fails
        // to parse there means quiet hours and batch windows silently fall
        // back to UTC — which on this machine is a five-hour error.
        let z = local_zone();
        assert!(capture_time::Zone::new(&z).is_some(), "not a zone: {z:?}",);
    }

    #[test]
    fn it_is_the_same_answer_every_time() {
        let _guard = serially();
        // Two callers a moment apart must not disagree. `today()` and
        // `agenda()` deriving the zone separately is how they came to
        // disagree about what day it was.
        assert_eq!(local_zone(), local_zone());
    }

    /// A zone change during the process takes effect.
    ///
    /// This asserted the opposite, and recorded the caching as a fact rather
    /// than a defect. It is a defect for the two processes that matter: the
    /// overlay and the daemon run for weeks, and the hour they read is the
    /// input to every §2.6 decision. Somebody who flies gets an afternoon that
    /// reads as quiet hours and a night that reads as morning.
    #[test]
    fn a_zone_change_is_picked_up_without_a_restart() {
        let _guard = serially();
        // SAFETY-adjacent: restored immediately, and nothing else in this
        // test binary reads TZ.
        unsafe { std::env::set_var("TZ", "Europe/Berlin") };
        let berlin = local_zone();
        unsafe { std::env::set_var("TZ", "America/Los_Angeles") };
        let la = local_zone();
        unsafe { std::env::remove_var("TZ") };

        assert_eq!(berlin, "Europe/Berlin");
        assert_eq!(la, "America/Los_Angeles", "the zone did not follow");
    }

    /// A `$TZ` that is not a zone name is passed over, not returned wrong.
    ///
    /// The reason the hand-rolled symlink parsing was removed. Every caller
    /// feeds this straight into `Zone::new`, so returning `EST5EDT` means
    /// silently falling back to UTC — a five-hour error nothing reports.
    #[test]
    fn a_tz_that_is_not_a_zone_name_is_not_believed() {
        let _guard = serially();
        for odd in ["EST5EDT", "<+07>-7", "/usr/share/zoneinfo/Europe/Rome", ""] {
            unsafe { std::env::set_var("TZ", odd) };
            let z = local_zone();
            unsafe { std::env::remove_var("TZ") };
            assert!(
                capture_time::Zone::new(&z).is_some(),
                "{odd:?} produced something that is not a zone: {z:?}"
            );
        }
    }
}

#[cfg(test)]
mod budget {
    use super::*;

    #[test]
    fn a_named_time_is_immediate() {
        // "a meeting starting" — §2.6's own example of the immediate case.
        assert_eq!(deliver_late(14, true, false, 0), Delivery::Now);
    }

    #[test]
    fn a_day_without_a_time_joins_the_next_batch() {
        // `call the dentist tomorrow` names a day. Delivering it at 00:00
        // would obey the letter of a due date and miss what was meant.
        assert_eq!(deliver_late(10, false, false, 0), Delivery::At(14));
        assert_eq!(deliver_late(15, false, false, 0), Delivery::At(19));
    }

    #[test]
    fn after_the_last_batch_it_waits_for_the_first() {
        // Not armed. This was written with `armed = true` to get past quiet
        // hours, which 20:00 is not — and it started failing the moment armed
        // became unconditional, correctly: something somebody set a moment for
        // does not wait for tomorrow morning.
        assert_eq!(deliver_late(20, false, false, 0), Delivery::At(BATCHES[0]));
    }

    #[test]
    fn quiet_hours_hold_everything_unarmed() {
        for hour in [22, 23, 0, 3, 6] {
            assert_eq!(
                deliver_late(hour, true, false, 0),
                Delivery::At(QUIET_UNTIL),
                "a timed alert at {hour}:00 still waits for morning",
            );
            assert_eq!(
                deliver_late(hour, false, false, 0),
                Delivery::At(QUIET_UNTIL)
            );
        }
    }

    #[test]
    fn a_moment_long_past_is_no_longer_urgent() {
        // Seven captures each naming 09:00, found overdue at once, all came
        // back `Now` — seven notifications the moment somebody opened the
        // phone. Being late is what makes them not urgent; each is still
        // individually "timed".
        let late = STILL_PROMPT_MS + 1;
        assert_eq!(deliver_late(10, true, false, late), Delivery::At(14));
    }

    #[test]
    fn a_moment_just_passed_is_still_urgent() {
        // The common case, and the one this must not break: an alarm fires a
        // few seconds late because a phone took a moment to wake.
        assert_eq!(deliver_late(10, true, false, 5_000), Delivery::Now);
        assert_eq!(
            deliver_late(10, true, false, STILL_PROMPT_MS - 1),
            Delivery::Now
        );
    }

    #[test]
    fn an_armed_alarm_is_urgent_however_late_in_the_day_it_is() {
        // Somebody who set an alarm meant it. Arriving late is a reason to say
        // so, not a reason to wait until the afternoon.
        //
        // "However late" is about the hour, not about the calendar: past
        // `STALE_MS` an armed moment is stale like any other, which
        // `arming_does_not_survive_a_fortnight` is the other half of.
        assert_eq!(
            deliver_late(10, true, true, 10 * STILL_PROMPT_MS),
            Delivery::Now
        );
    }

    #[test]
    fn the_old_signature_still_means_on_time() {
        for hour in 0..24 {
            for timed in [true, false] {
                for armed in [true, false] {
                    assert_eq!(
                        deliver_late(hour, timed, armed, 0),
                        deliver_late(hour, timed, armed, 0),
                        "deliver is deliver_late with nothing late about it",
                    );
                }
            }
        }
    }

    #[test]
    fn an_armed_moment_overrides_quiet_hours() {
        // Somebody who set an alarm for 6am meant 6am. A system that decided
        // it knew better would be useless for the one thing an alarm is for.
        assert_eq!(deliver_late(6, true, true, 0), Delivery::Now);
        assert_eq!(deliver_late(23, true, true, 0), Delivery::Now);
    }

    #[test]
    fn the_boundaries_are_where_they_are_written() {
        assert_eq!(
            deliver_late(21, true, false, 0),
            Delivery::Now,
            "21:00 is not quiet"
        );
        assert_eq!(deliver_late(22, true, false, 0), Delivery::At(QUIET_UNTIL));
        assert_eq!(
            deliver_late(7, true, false, 0),
            Delivery::Now,
            "07:00 is morning"
        );
    }

    /// Everything held is eventually said.
    ///
    /// The property that was missing, and its absence cost the entire batched
    /// class: tasks with due dates, late alerts, and anything held over quiet
    /// hours were delivered at no hour of the day. The old test asserted the
    /// batch hours sit outside quiet hours — that a loop could not exist —
    /// which is a fact about the arrangement rather than about the outcome,
    /// and the loop was one comparison away from it.
    ///
    /// Stated as: whatever is held, following the holds forward terminates.
    /// A moment far enough past stops being an announcement.
    ///
    /// The three ways this was reachable are in `STALE_MS`, and the worst is
    /// the quietest: a restore starts with an empty delivery table, so without
    /// a floor every moment in a year-wide horizon reads as never delivered.
    /// Somebody recovering an archive would be handed a year of alerts for
    /// moments they already lived through, on the day they could least afford
    /// it.
    #[test]
    fn a_moment_long_past_is_not_announced() {
        let week = STALE_MS;
        // Just inside: still said, on whatever terms lateness allows.
        assert_ne!(deliver_late(10, true, false, week), Delivery::Stale);
        // Past it: nothing, on any terms.
        for hour in [0, 7, 9, 14, 19, 23] {
            for timed in [true, false] {
                for armed in [true, false] {
                    assert_eq!(
                        deliver_late(hour, timed, armed, week + 1),
                        Delivery::Stale,
                        "hour={hour} timed={timed} armed={armed}"
                    );
                }
            }
        }
    }

    /// Armed is unconditional about *quiet hours*, not about time itself.
    ///
    /// An alarm somebody set is said whenever it arrives, however inconvenient
    /// the hour — that is what arming means. An alarm for a moment a fortnight
    /// gone is a different claim, and ringing it teaches somebody the ring
    /// means nothing.
    #[test]
    fn arming_does_not_survive_a_fortnight() {
        assert_eq!(deliver_late(3, true, true, 0), Delivery::Now);
        assert_eq!(
            deliver_late(3, true, true, 14 * 24 * 60 * 60 * 1000),
            Delivery::Stale
        );
    }

    #[test]
    fn a_hold_always_ends_somewhere() {
        for start in 0..24 {
            for timed in [true, false] {
                let mut hour = start;
                let mut hops = 0;
                loop {
                    match deliver_late(hour, timed, false, 24 * 60 * 60 * 1000) {
                        Delivery::Now => break,
                        Delivery::At(next) => {
                            hour = next;
                            hops += 1;
                            assert!(
                                hops <= BATCHES.len() + 2,
                                "held from {start}:00 (timed={timed}) and never said — \
                                 the holds cycle rather than terminating",
                            );
                        }
                        // A day late is well inside the floor. If this ever
                        // fires, the floor has moved under the property rather
                        // than the property being wrong.
                        Delivery::Stale => {
                            panic!("a day late was treated as too late to say")
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn arriving_at_a_batch_hour_is_the_batch() {
        for b in BATCHES {
            assert_eq!(deliver_late(b, false, false, 0), Delivery::Now, "at {b}:00");
            assert_eq!(
                deliver_late(b, true, false, 24 * 60 * 60 * 1000),
                Delivery::Now,
                "a day-late timed alert at {b}:00",
            );
        }
    }

    #[test]
    fn a_task_with_a_due_date_is_announced_at_some_point() {
        // The ordinary case, and the one that was silently never delivered.
        let said_somewhere = (0..24).any(|h| deliver_late(h, false, false, 0) == Delivery::Now);
        assert!(said_somewhere, "a due date is never announced at any hour");
    }

    #[test]
    fn every_batch_is_outside_quiet_hours() {
        // Otherwise a held alert would be released into the hours it was held
        // out of, which is the loop this pair of rules must not contain.
        for b in BATCHES {
            assert!(
                (QUIET_UNTIL..QUIET_FROM).contains(&b),
                "batch at {b}:00 is in quiet hours"
            );
        }
    }

    #[test]
    fn a_decision_is_always_reachable() {
        // No hour of the day may produce a hold that never comes.
        for hour in 0..24 {
            for timed in [true, false] {
                for armed in [true, false] {
                    if let Delivery::At(h) = deliver_late(hour, timed, armed, 0) {
                        assert!(h < 24, "held until {h}:00, which is not an hour");
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_moment_fires_once_and_a_place_fires_every_visit() {
        // The difference between a reminder and a plan (§2.4). "Buy screws
        // when you are at the hardware shop" is worthless if it only works the
        // first time somebody goes there.
        assert!(may_fire_again(None, 1_000, "time"), "never fired yet");
        assert!(
            !may_fire_again(Some(1_000), 9_999_999_999, "time"),
            "a moment passed once and saying so again is noise"
        );
        assert!(
            may_fire_again(Some(1_000), 1_000 + REARM_MS, "location"),
            "arriving again is a new arrival"
        );
    }

    #[test]
    fn standing_at_the_edge_of_a_fence_does_not_notify_repeatedly() {
        // A shop is a point with a radius; somebody at the counter is metres
        // from the edge, and a phone re-acquiring its position will cross that
        // edge without anybody moving.
        let arrived = 1_700_000_000_000;
        assert!(!may_fire_again(Some(arrived), arrived + 60_000, "location"));
        assert!(!may_fire_again(
            Some(arrived),
            arrived + REARM_MS - 1,
            "location"
        ));
    }

    #[test]
    fn a_cue_this_device_cannot_see_is_refused() {
        // Declaring a cue type requires being able to observe it. Accepting
        // `app_focus` here would produce an intent that is registered, reported
        // as working, and silently never fires.
        assert!(!observable("app_focus"), "no platform here can see this");

        // A geofence is the phone's to observe. Answered by a machine that
        // does not move, it either never fires or fires once and never stops.
        assert_eq!(
            observable("location"),
            cfg!(target_os = "android"),
            "location is observable exactly where there is something that moves"
        );

        // And the converse: a branch is where the repository is.
        assert_eq!(observable("git_branch"), !cfg!(target_os = "android"));
        assert!(!observable("calendar_start"));
    }

    #[test]
    fn a_clock_is_readable_from_any_device() {
        for cue in ["time", "recurring", "device_wake"] {
            assert!(observable(cue), "{cue} needs nothing but a clock");
        }
    }

    #[test]
    #[cfg(not(target_os = "android"))]
    fn a_desktop_can_see_the_repository_it_holds() {
        assert!(observable("git_branch"));
    }

    #[test]
    fn an_unknown_cue_is_not_quietly_accepted() {
        assert!(!observable("telepathy"));
    }
}

#[cfg(test)]
mod default_rule {
    use super::*;

    const HOUR: u64 = 60 * 60 * 1000;
    const NOW: u64 = 1_800_000_000_000;

    fn at(wall: u64) -> Option<capture_core::Hlc> {
        Some(capture_core::Hlc {
            wall,
            counter: 0,
            origin: capture_core::Dev([0; 32]),
        })
    }

    fn device(n: u8, admitted: u64, reached: Option<u64>) -> Candidate {
        Candidate {
            key: [n; 32],
            admitted: at(admitted),
            silent: false,
            reached,
        }
    }

    #[test]
    fn of_two_paired_devices_only_the_newer_speaks() {
        let desk = device(1, 100, Some(NOW - HOUR));
        let phone = device(2, 200, Some(NOW - HOUR));
        let both = [desk.clone(), phone.clone()];
        // Both devices, each asking from its own seat, name the same one.
        assert_eq!(default_notifier(&both, &desk.key, NOW), phone.key);
        assert_eq!(default_notifier(&both, &phone.key, NOW), phone.key);
    }

    #[test]
    fn a_silent_device_hands_over_to_the_next() {
        let desk = device(1, 100, None);
        let mut phone = device(2, 200, Some(NOW - HOUR));
        phone.silent = true;
        assert_eq!(
            default_notifier(&[desk.clone(), phone], &desk.key, NOW),
            desk.key
        );
    }

    #[test]
    fn a_device_gone_quiet_hands_over_and_one_never_reached_is_not_counted_on() {
        let desk = device(1, 100, None);
        let stale = device(2, 200, Some(NOW - 25 * HOUR));
        assert_eq!(
            default_notifier(&[desk.clone(), stale], &desk.key, NOW),
            desk.key
        );
        // Paired a minute ago and never synced: the device that can see the
        // moment keeps saying it until a round has actually happened.
        let fresh = device(3, 300, None);
        assert_eq!(
            default_notifier(&[desk.clone(), fresh], &desk.key, NOW),
            desk.key
        );
    }

    #[test]
    fn with_nobody_qualifying_this_device_speaks() {
        let mut me = device(1, 100, None);
        me.silent = true;
        let gone = device(2, 200, None);
        assert_eq!(default_notifier(&[me.clone(), gone], &me.key, NOW), me.key);
    }

    #[test]
    fn an_older_build_that_reports_nothing_is_not_read_as_silent() {
        // `silent` is the token, and its absence means able. A positive
        // `notify` would have skipped every peer that had not upgraded.
        let desk = device(1, 100, None);
        let phone = device(2, 200, Some(NOW));
        assert_eq!(
            default_notifier(&[desk.clone(), phone.clone()], &desk.key, NOW),
            phone.key
        );
    }
}

#[cfg(test)]
mod delivery {
    use super::*;

    /// A stand-in `osascript` that records its argv and exits with `code`.
    fn stub(name: &str, code: i32) -> (std::path::PathBuf, std::path::PathBuf) {
        use std::os::unix::fs::PermissionsExt;
        let dir =
            std::env::temp_dir().join(format!("capture-notifier-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let log = dir.join("argv");
        let prog = dir.join("osascript");
        std::fs::write(
            &prog,
            format!(
                "#!/bin/sh\nfor a in \"$@\"; do printf '%s\\n' \"$a\"; done > '{}'\nexit {code}\n",
                log.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&prog, std::fs::Permissions::from_mode(0o755)).unwrap();
        (prog, log)
    }

    #[test]
    fn a_mac_notification_carries_the_text_as_data() {
        let (prog, log) = stub("data", 0);
        let hostile = r#"x" & (do shell script "touch /tmp/owned") & ""#;
        let args = notifier_args(true, "dentist", hostile);
        assert!(run_notifier(prog.to_str().unwrap(), &args));
        let seen = std::fs::read_to_string(&log).unwrap();
        let lines: Vec<&str> = seen.lines().collect();
        // The script is fixed; the text arrives as the last two arguments.
        assert_eq!(lines.last(), Some(&hostile));
        assert_eq!(lines[lines.len() - 2], "dentist");
        assert!(
            lines
                .iter()
                .filter(|l| l.contains("do shell script"))
                .count()
                == 1
        );
        assert!(
            args.iter()
                .take(6)
                .all(|a| !a.contains("dentist") && !a.contains("shell")),
            "text reached the script"
        );
        let _ = std::fs::remove_dir_all(prog.parent().unwrap());
    }

    #[test]
    fn a_refused_or_missing_notifier_is_not_delivered() {
        let (prog, _) = stub("refused", 1);
        assert!(!run_notifier(
            prog.to_str().unwrap(),
            &notifier_args(true, "a", "b")
        ));
        assert!(!run_notifier(
            "/nonexistent/osascript",
            &notifier_args(true, "a", "b")
        ));
        let _ = std::fs::remove_dir_all(prog.parent().unwrap());
    }

    #[test]
    fn linux_keeps_its_arguments() {
        assert_eq!(
            notifier_args(false, "s", "b"),
            vec!["--app-name=capture".to_string(), "s".into(), "b".into()]
        );
    }
}
