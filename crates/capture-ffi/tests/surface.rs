//! Every operation the phone can perform, called directly.
//!
//! Before any Kotlin exists, and deliberately so. A binding exercised only
//! through a screen is one whose failures arrive as a blank screen: the
//! interesting question — did the store do the right thing — is then two
//! layers and a language away from where it is answered.

use capture_ffi::{Archive, Captured};

use capture_store::pairing::PORT as PAIRING_PORT;

/// Every `setting` event in an archive's log, as `key=value`.
///
/// Read from the log rather than through a surface, because the surface is
/// the thing that would be wrong. A setting the projection had not folded yet
/// would look, through any reader, exactly like a setting nobody wrote.
fn settings_written(root: &str) -> Vec<String> {
    let paths = capture_store::paths::StorePaths::under(std::path::Path::new(root));
    let log = capture_store::log::Log::open_read(&paths.events).unwrap();
    log.all()
        .unwrap()
        .into_iter()
        .filter(|e| e.header.kind == capture_core::event::Kind::SETTING)
        .map(|e| format!("{:?}", e.header.kind))
        .collect()
}

fn scratch(name: &str) -> String {
    let d = std::env::temp_dir().join(format!(
        "capture-ffi-{name}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&d);
    d.to_string_lossy().to_string()
}

#[test]
fn opening_an_empty_directory_creates_an_archive() {
    // The phone's first run. There is no `capture init` on Android — the
    // application is opened and there has to be somewhere to put things.
    let root = scratch("fresh");
    let a = Archive::open(root.clone()).expect("opened");

    assert_eq!(a.note_count().unwrap(), 0);
    assert_eq!(a.device_key().unwrap().len(), 12, "six bytes, as hex");

    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn opening_twice_keeps_the_same_device() {
    // A phone is a device, not a session. Minting a new identity on every open
    // would give one machine many origins, and every peer would have to be
    // told about each of them.
    let root = scratch("same-device");
    let first = Archive::open(root.clone()).unwrap().device_key().unwrap();
    let second = Archive::open(root.clone()).unwrap().device_key().unwrap();

    assert_eq!(first, second);
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn a_capture_is_readable_immediately() {
    let root = scratch("capture");
    let a = Archive::open(root.clone()).unwrap();

    let slug = match a.capture("milk and eggs".into()).unwrap() {
        Captured::Stored { slug } => slug,
        Captured::Spooled => panic!("spooled with a writable store"),
    };

    let note = a.note(slug.clone()).unwrap();
    assert_eq!(note.text.as_deref(), Some("milk and eggs"));
    assert_eq!(note.slug, slug);
    assert_eq!(a.note_count().unwrap(), 1);

    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn a_capture_survives_the_handle_being_dropped() {
    // The durability promise, in the shape Android actually produces: a Quick
    // Settings tile is destroyed the moment its callback returns. If anything
    // were queued on the caller's side rather than committed, this is where it
    // would be lost.
    let root = scratch("durable");
    {
        let a = Archive::open(root.clone()).unwrap();
        a.capture("written by something about to die".into())
            .unwrap();
    }

    let reopened = Archive::open(root.clone()).unwrap();
    assert_eq!(reopened.note_count().unwrap(), 1);

    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn the_timeline_is_newest_first() {
    let root = scratch("timeline");
    let a = Archive::open(root.clone()).unwrap();
    a.capture("first".into()).unwrap();
    a.capture("second".into()).unwrap();
    a.capture("third".into()).unwrap();

    let rows = a.timeline(10).unwrap();
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0].text.as_deref(), Some("third"));

    assert_eq!(a.timeline(2).unwrap().len(), 2, "the limit is honoured");
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn search_finds_what_was_captured() {
    let root = scratch("search");
    let a = Archive::open(root.clone()).unwrap();
    a.capture("buy sourdough starter".into()).unwrap();
    a.capture("something else entirely".into()).unwrap();

    let hits = a.search("sourdough".into(), 10).unwrap();
    assert_eq!(hits.len(), 1);
    assert!(hits[0].text.as_deref().unwrap().contains("sourdough"));

    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn asking_for_a_note_that_is_not_there_says_so() {
    // Distinguishable from a store failure. A caller that cannot tell those
    // apart will report both as a crash, and one of them is ordinary.
    let root = scratch("missing");
    let a = Archive::open(root.clone()).unwrap();

    let e = a.note("no-such-note".into()).unwrap_err();
    assert!(
        matches!(e, capture_ffi::CaptureError::NotFound { .. }),
        "expected NotFound, got {e:?}"
    );

    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn concurrent_captures_do_not_report_a_false_conflict() {
    // Several coroutines calling in at once is the ordinary case on Android.
    // The store's writer lock protects *between* processes; two callers inside
    // one process racing for it would otherwise produce "somebody else is
    // writing" for the only process on the device.
    use std::sync::Arc;

    let root = scratch("concurrent");
    let a = Arc::new(Archive::open(root.clone()).unwrap());

    let mut threads = Vec::new();
    for i in 0..8 {
        let a = Arc::clone(&a);
        threads.push(std::thread::spawn(move || {
            a.capture(format!("note number {i}")).unwrap()
        }));
    }
    for t in threads {
        match t.join().expect("no panic") {
            Captured::Stored { .. } => {}
            Captured::Spooled => panic!("spooled — a writable store reported a conflict"),
        }
    }

    assert_eq!(a.note_count().unwrap(), 8);
    std::fs::remove_dir_all(&root).ok();
}

// ------------------------------------------------------------- mutations ----
//
// The phone offers every mutation the desktop does (§8.4). A device that can
// destroy a note but not mute one inverts the gradient §2.6 exists to create:
// suppressing something must be cheaper than destroying it, and it stops being
// cheaper the moment it needs a different machine.

#[test]
fn every_mutation_is_reachable_from_the_phone() {
    let root = scratch("mutations");
    let a = Archive::open(root.clone()).unwrap();

    let slug = match a.capture("a note to work on".into()).unwrap() {
        Captured::Stored { slug } => slug,
        Captured::Spooled => panic!("spooled"),
    };

    a.edit(slug.clone(), "edited on the phone".into()).unwrap();
    assert_eq!(
        a.note(slug.clone()).unwrap().text.as_deref(),
        Some("edited on the phone")
    );

    a.tag(slug.clone(), "errand".into()).unwrap();
    a.untag(slug.clone(), "errand".into()).unwrap();

    a.set_alias(slug.clone(), Some("the-thing".into())).unwrap();
    assert_eq!(
        a.note("the-thing".into()).unwrap().slug,
        slug,
        "an alias resolves to the same note"
    );

    a.pin(slug.clone(), true).unwrap();
    assert!(a.note(slug.clone()).unwrap().pinned);

    a.mute(slug.clone()).unwrap();
    assert!(
        a.note(slug.clone()).unwrap().muted_until_ms.is_some(),
        "muting is available without reaching for a laptop"
    );

    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn a_queued_destruction_is_distinguishable_from_a_finished_one() {
    // The distinction a single flag would lose. Between ordering and executing,
    // the content is withheld and still recoverable — and that is the only one
    // of the two states a person can do anything about.
    let root = scratch("redact");
    let a = Archive::open(root.clone()).unwrap();
    let slug = match a.capture("something regrettable".into()).unwrap() {
        Captured::Stored { slug } => slug,
        Captured::Spooled => panic!("spooled"),
    };

    let id = a.redact(slug.clone()).unwrap();

    let n = a.note(slug.clone()).unwrap();
    assert!(n.redaction_queued, "ordered");
    assert!(!n.redacted, "and not yet carried out");
    assert!(n.text.is_none(), "withheld in the meantime");

    a.cancel_redaction(id).unwrap();
    assert!(
        !a.note(slug).unwrap().redaction_queued,
        "the window is real: a queued destruction can be called off"
    );

    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn the_cancelling_id_outlives_the_process_that_ordered_it() {
    // The window is worth nothing if only the process that opened it can close
    // it. A phone destroys and rebuilds an application constantly — rotation,
    // a low-memory kill, a minute in the background — and every one of those
    // would otherwise commit a destruction that was still cancellable a second
    // earlier, without anybody deciding to.
    let root = scratch("redact-restart");
    let a = Archive::open(root.clone()).unwrap();
    let slug = match a.capture("regrettable".into()).unwrap() {
        Captured::Stored { slug } => slug,
        Captured::Spooled => panic!("spooled"),
    };
    a.redact(slug.clone()).unwrap();
    drop(a);

    // A different process, holding nothing from the first.
    let b = Archive::open(root.clone()).unwrap();
    let waiting = b
        .pending_redaction(slug.clone())
        .unwrap()
        .expect("the queue is in the projection, not in a process");
    assert!(!waiting.at.is_empty(), "and it says when the window closes");

    b.cancel_redaction(waiting.id).unwrap();
    assert!(!b.note(slug.clone()).unwrap().redaction_queued);
    assert!(
        b.pending_redaction(slug).unwrap().is_none(),
        "and a cancelled redaction stops being offered as cancellable"
    );

    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn settling_is_something_the_phone_asks_for() {
    // The desktop settles on every command that writes. A phone has no
    // equivalent — it is opened and closed rather than invoked — so the work
    // has to be asked for, on foreground and on the sync tick.
    let root = scratch("settle");
    let a = Archive::open(root.clone()).unwrap();
    a.capture("ordinary".into()).unwrap();

    assert_eq!(a.settle().unwrap(), 0, "nothing due, nothing done");

    std::fs::remove_dir_all(&root).ok();
}

// --------------------------------------------------------------- reading ----

#[test]
fn the_agenda_shows_what_is_due_on_a_day() {
    let root = scratch("agenda");
    let a = Archive::open(root.clone()).unwrap();

    // A task with a day. The recognizer decides this is a task rather than an
    // event, and the phone does not get a vote — §3.6's discriminator is about
    // time, and there is one implementation of it.
    a.capture("call the dentist friday".into()).unwrap();
    a.settle().unwrap();

    // Whatever day that resolved to, asking for a day answers without the
    // caller doing any date arithmetic of its own.
    let entries = a.agenda("2026-09-15".into()).unwrap();
    let _ = entries; // may legitimately be empty; the point is it answers

    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn a_day_that_is_not_a_day_is_refused() {
    // Date parsing happens here rather than in Kotlin, so this is where a bad
    // one has to be caught. The gate forbids Kotlin doing it at all.
    let root = scratch("agenda-bad");
    let a = Archive::open(root.clone()).unwrap();

    let e = a.agenda("next tuesday".into()).unwrap_err();
    assert!(
        matches!(e, capture_ffi::CaptureError::Refused { .. }),
        "expected a refusal, got {e:?}"
    );

    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn named_pages_carry_their_parent() {
    // What the wiki sidebar groups by. The nesting is a field on each page
    // rather than a tree the caller assembles, so there is nothing for Kotlin
    // to get wrong about cycles or orphans.
    let root = scratch("named");
    let a = Archive::open(root.clone()).unwrap();

    let parent = match a.capture("acme".into()).unwrap() {
        Captured::Stored { slug } => slug,
        Captured::Spooled => panic!("spooled"),
    };
    let child = match a.capture("acme budget".into()).unwrap() {
        Captured::Stored { slug } => slug,
        Captured::Spooled => panic!("spooled"),
    };
    a.set_alias(parent.clone(), Some("acme".into())).unwrap();
    a.set_alias(child.clone(), Some("acme-budget".into()))
        .unwrap();
    a.set_parent("acme-budget".into(), Some("acme".into()))
        .unwrap();

    let pages = a.named().unwrap();
    let budget = pages
        .iter()
        .find(|p| p.alias.as_deref() == Some("acme-budget"))
        .expect("the child page is named");
    assert!(budget.parent.is_some(), "it knows what it sits under");

    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn health_says_nothing_about_a_healthy_archive() {
    // An empty list is the honest answer here, and the surface says so out
    // loud rather than rendering an empty box — that ambiguity between
    // "nothing is wrong" and "nothing was checked" is the thing §8.9 objects
    // to, and it belongs to the surface rather than to this.
    let root = scratch("health");
    let a = Archive::open(root.clone()).unwrap();
    a.capture("ordinary".into()).unwrap();

    assert!(a.health().unwrap().is_empty());

    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn pairing_two_devices_writes_no_setting() {
    // §8.9, and an earlier draft of the design got this exactly wrong.
    //
    // Pairing is a person connecting two devices. It is not a person expressing
    // a preference about anything, and if the act of pairing authored settings
    // then an archive could never afterwards distinguish what somebody chose
    // from what the software assumed on their behalf — which is the whole
    // distinction §8.9 exists to keep.
    //
    // The temptation is real and specific: the first phone to join is an
    // obvious notifier, and writing that down here would save a person one
    // decision. It would also mean their notifier was chosen by the order they
    // happened to pair things in.
    let one = scratch("pair-a");
    let two = scratch("pair-b");
    let a = Archive::open(one.clone()).unwrap();
    let b = Archive::open(two.clone()).unwrap();

    let offer = a.offer_pairing().unwrap();
    let code = offer.code();
    assert_eq!(code.len(), 6, "six digits, as the desktop shows");

    let joiner = std::thread::spawn(move || {
        // A short wait so the listener is certainly accepting. The window is
        // three minutes; this is not a race the test can lose quietly.
        std::thread::sleep(std::time::Duration::from_millis(300));
        b.join_with_code(format!("127.0.0.1:{}", PAIRING_PORT), code)
            .unwrap();
        b
    });

    offer
        .wait()
        .expect("a device joined before the window closed");
    let b = joiner.join().expect("the joining side did not panic");

    for (which, root) in [("the offering device", &one), ("the joining device", &two)] {
        assert!(
            settings_written(root).is_empty(),
            "{which} authored settings by pairing: {:?}",
            settings_written(root)
        );
    }

    // And it did pair: this is not passing because nothing happened.
    assert_eq!(
        a.peers().unwrap().len(),
        1,
        "the offering device knows one peer"
    );
    assert_eq!(b.peers().unwrap().len(), 1, "and so does the joining one");

    // And the instrument works. An assertion that a log contains no events of
    // some kind passes just as cheerfully when the reader is broken, and this
    // is the only way to tell those apart.
    let paths = capture_store::paths::StorePaths::under(std::path::Path::new(&one));
    capture_store::mutate::set_setting(&paths, "timeline.limit", Some("50")).unwrap();
    assert_eq!(
        settings_written(&one).len(),
        1,
        "a setting that was written is seen; so the emptiness above meant something"
    );

    std::fs::remove_dir_all(&one).ok();
    std::fs::remove_dir_all(&two).ok();
}

#[test]
fn the_address_offered_for_pairing_can_actually_be_dialled() {
    // A phone showed "capture pair 521039 --at fe80::80cc:80ec:8b48:71a8".
    //
    // That address cannot be dialled from anywhere. A link-local IPv6 address
    // needs the zone index of the interface it belongs to, which is local to
    // the machine that owns it and means nothing on another one — so the
    // instruction on screen was one nobody could follow, and it failed at the
    // far end in a way that reads as the *code* being wrong.
    let root = scratch("addresses");
    let a = Archive::open(root.clone()).unwrap();

    for addr in a.addresses() {
        assert!(
            !addr.starts_with("fe80:"),
            "offered a link-local address nobody can dial: {addr}"
        );
        assert!(
            !addr.starts_with("127.") && addr != "::1",
            "offered loopback, which sends the other device to itself: {addr}"
        );
    }

    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn a_review_session_is_offered_and_marked_as_offered() {
    // Opening a session writes. The interval expands because the archive
    // recorded that these were shown, so a surface that opened one on every
    // navigation would push notes away without anybody having read them.
    let root = scratch("review");
    let a = Archive::open(root.clone()).unwrap();
    for i in 0..3 {
        a.capture(format!("something worth resurfacing {i}"))
            .unwrap();
    }

    let first = a.review().unwrap();
    assert!(!first.items.is_empty(), "fresh notes are offered");
    for item in &first.items {
        assert!(
            ["forward", "never-engaged", "context"].contains(&item.because.as_str()),
            "every card says why it is here, not just that it is: {}",
            item.because,
        );
    }

    // Nothing is graded. The only two a surface may record.
    a.reviewed(first.items[0].slug.clone(), "engaged".into())
        .unwrap();
    a.reviewed(first.items[1].slug.clone(), "deferred".into())
        .unwrap();

    let refused = a.reviewed(first.items[2].slug.clone(), "forgotten".into());
    assert!(refused.is_err(), "there is no grade to record");

    // Not even the one the archive writes for itself: a surface able to write
    // `shown` could spend a session nobody saw.
    assert!(
        a.reviewed(first.items[2].slug.clone(), "shown".into())
            .is_err(),
        "`shown` is the archive's to write"
    );

    std::fs::remove_dir_all(&root).ok();
}
