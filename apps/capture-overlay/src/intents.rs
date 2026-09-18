//! Watching for the cues this device can actually see.
//!
//! # Local cues fire where they happen
//!
//! §2.4.1 splits the cues in two, and the split is not about load balancing.
//! Enrichment devices are interchangeable; notification devices are not. "Check
//! the incidents backlog when you open Slack" delivered to a phone while you
//! are sitting at the desktop is worthless however fairly it was assigned.
//!
//! So `git_branch` fires on the machine the repository is on, and that machine
//! is this one. Time cues go to the designated notifier, which — until there is
//! a second device to choose between — is also this one, by a rule rather than
//! by a setting anybody wrote (§8.9).
//!
//! # Fired once, remembered locally
//!
//! Whether a notification was delivered is operational chatter rather than a
//! fact about the archive, so it lives in local state (§2.5.2). Two devices can
//! each fire their own local cues without arguing, and a log meant to be worth
//! reading in ten years does not fill up with "a toast appeared".

use capture_store::{StorePaths, notifier};

/// Look once for anything that has come true.
///
/// Called from the same tick that drains due work. Cheap: one indexed query and
/// a branch read, against a store that usually has no intents at all.
pub fn check(paths: &StorePaths, now: i64) {
    if let Err(e) = look(paths, now) {
        tracing::warn!(%e, "intents could not be checked");
    }
}

fn look(paths: &StorePaths, now: i64) -> capture_store::Result<()> {
    let generation = capture_store::ops::Ops::open_read(&paths.ops)?.generation()?;
    let state = capture_store::projection::Store::open_read(
        &capture_store::projection::generation_path(&paths.state, generation),
    )?;

    let branch = current_branch();

    for i in state.intents()? {
        if notifier::already_fired(paths, &i.id)? {
            continue;
        }

        let fires = match i.cue_type.as_str() {
            // A moment that has arrived. Overdue counts: a machine that was
            // switched off through 3pm should say so when it comes back rather
            // than pretending the afternoon did not happen.
            "time" => i.cue_at.is_some_and(|at| at <= now),

            // Only where the repository actually is. Sending this to another
            // device would destroy the point of the cue.
            "git_branch" => branch.as_deref() == i.cue_match.as_deref(),

            // Everything else is either not observable here or has no
            // producer yet, and `add_intent` refuses to create those at all.
            _ => false,
        };

        if !fires {
            continue;
        }

        if notifier::notify("brain", &i.then_do) {
            notifier::mark_fired(paths, &i.id, now.max(0) as u64)?;
        }
        tracing::info!(cue = %i.cue_type, "an intent fired");
    }

    alerts(paths, now)?;
    Ok(())
}

/// The moments captures named, rather than the plans somebody wrote.
///
/// An intent is a deliberate if-then; this is the ordinary case — `dentist
/// thursday 2pm` promoted to an event with a time. §3.1 makes the type a
/// promotion rather than a choice, and an alert that had to be armed
/// separately would put the second decision back that §2.1 spent the whole
/// capture gesture avoiding.
///
/// Subject to the same budget as everything else (§2.6). The decision belongs
/// to `notifier::deliver` rather than here, so the desktop and the phone
/// cannot come to different conclusions about what hour it is safe to speak.
fn alerts(paths: &StorePaths, now: i64) -> capture_store::Result<()> {
    use capture_store::notifier::{Delivery, deliver};

    let zone_name = local_zone();
    let Some(zone) = capture_time::Zone::new(&zone_name) else {
        return Ok(());
    };
    let hour = capture_time::civil_clock(now, zone).0;

    let generation = capture_store::ops::Ops::open_read(&paths.ops)?.generation()?;
    let state = capture_store::projection::Store::open_read(
        &capture_store::projection::generation_path(&paths.state, generation),
    )?;
    let ops = capture_store::ops::Ops::open(&paths.ops)?;

    for d in state.due_by(now, &zone_name)? {
        if ops.alerted(&d.tgt.0, d.occ)? {
            continue;
        }
        // Held, not dropped. Nothing is marked, so it is still due when the
        // hour comes round.
        if deliver(hour, d.timed, false) != Delivery::Now {
            continue;
        }

        // Late is said rather than hidden. A machine that was off through an
        // afternoon should say the afternoon happened.
        let late = now.saturating_sub(d.at) > 5 * 60 * 1000;
        let said = if late {
            format!("{} (was due earlier)", d.text)
        } else {
            d.text.clone()
        };
        if notifier::notify("capture", &said) {
            ops.mark_alerted(&d.tgt.0, d.occ, now.max(0) as u64)?;
        }
        tracing::info!("an alert fired");
    }

    Ok(())
}

/// The branch of the repository this process is sitting in, if any.
///
/// Read from `.git/HEAD` rather than by running git: this happens on a timer in
/// a resident process, and spawning a subprocess every minute to answer a
/// question that is one line of a file is not a trade worth making.
fn current_branch() -> Option<String> {
    let mut dir = std::env::current_dir().ok()?;
    loop {
        let head = dir.join(".git").join("HEAD");
        if let Ok(contents) = std::fs::read_to_string(&head) {
            return contents
                .trim()
                .strip_prefix("ref: refs/heads/")
                .map(String::from);
        }
        if !dir.pop() {
            return None;
        }
    }
}

/// The zone this machine is in, by name.
///
/// The same reading the command line does. `TZ` where it is set, otherwise
/// what `/etc/localtime` points at — there is no portable call for this and a
/// wrong answer here moves quiet hours by the offset, which is the difference
/// between a notification at 07:00 and one at 02:00.
fn local_zone() -> String {
    std::env::var("TZ")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| {
            std::fs::read_link("/etc/localtime").ok().and_then(|p| {
                p.to_str()
                    .and_then(|s| s.split("zoneinfo/").nth(1))
                    .map(String::from)
            })
        })
        .unwrap_or_else(|| "UTC".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_branch_is_read_from_the_repository_this_is_in() {
        // This test runs inside the project's own checkout, so there is one.
        let b = current_branch();
        assert!(b.is_some(), "expected to be inside a git repository");
        assert!(!b.unwrap().is_empty());
    }

    #[test]
    fn a_detached_head_is_not_a_branch() {
        // `.git/HEAD` holds a bare object id when detached, and reporting that
        // as a branch name would make a cue match a thing nobody named.
        let dir = std::env::temp_dir().join(format!("capture-head-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join(".git")).unwrap();
        std::fs::write(
            dir.join(".git").join("HEAD"),
            "9f1c2b3a4d5e6f708192a3b4c5d6e7f809a1b2c3\n",
        )
        .unwrap();

        let previous = std::env::current_dir().unwrap();
        std::env::set_current_dir(&dir).unwrap();
        let branch = current_branch();
        std::env::set_current_dir(previous).unwrap();
        let _ = std::fs::remove_dir_all(&dir);

        assert_eq!(branch, None);
    }
}
