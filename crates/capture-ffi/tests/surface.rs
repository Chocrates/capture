//! Every operation the phone can perform, called directly.
//!
//! Before any Kotlin exists, and deliberately so. A binding exercised only
//! through a screen is one whose failures arrive as a blank screen: the
//! interesting question — did the store do the right thing — is then two
//! layers and a language away from where it is answered.

use capture_ffi::{Archive, Captured, Watching};

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
    assert!(a.note(slug.clone()).unwrap().muted);

    // And back, from the same place. §2.6: reversible from any device — the
    // phone could mute and only a terminal could undo it.
    a.unmute(slug.clone()).unwrap();
    assert!(
        !a.note(slug.clone()).unwrap().muted,
        "unmuting from the phone did not lift the mute"
    );
    assert!(
        a.unmute(slug.clone()).is_err(),
        "unmuting a note that is not muted wrote something anyway"
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

#[test]
fn a_plan_about_a_place_that_cannot_fire_says_so() {
    // §7.3. An intent failing invisibly is worse than no intent: the person
    // believes something is watching for them, stops holding it in mind, and
    // that is the whole thing this exists to do.
    let root = scratch("unfirable");
    let a = Archive::open(root.clone()).unwrap();

    // Watching a place is refused where the device cannot observe one, which
    // is every device that does not move — so on a desktop there is nothing to
    // report and this asserts the refusal instead.
    let watching = a.watch_place(51_507_351, -128_099, 100, "buy screws".into());

    if cfg!(target_os = "android") {
        let id = watching.expect("a phone can watch a place");

        a.location_permitted(Watching::Never).unwrap();
        let complaints = a.health().unwrap();
        assert!(
            complaints
                .iter()
                .any(|c| c.tier == "attention" && c.text.contains("cannot fire")),
            "an intent nothing is watching for is not a quiet detail: {complaints:?}"
        );

        // The trap in the middle. Android offers "while using the app", which
        // reads like a smaller permission and is none at all for this: nobody
        // is looking at a notes application while walking into a shop. The
        // emulator granted exactly this, the fences registered, and nothing
        // reported a thing — which is §7.3's failure wearing the disguise of
        // having worked.
        a.location_permitted(Watching::WhileOpen).unwrap();
        let complaints = a.health().unwrap();
        assert!(
            complaints
                .iter()
                .any(|c| c.text.contains("only fire while capture is open")),
            "a permission that cannot deliver is not a permission: {complaints:?}"
        );

        a.location_permitted(Watching::Always).unwrap();
        assert!(
            !a.health()
                .unwrap()
                .iter()
                .any(|c| c.text.contains("plan(s) about a place")),
            "and it stops being reported once it can actually fire"
        );

        // It fires on arrival, and not again while somebody stands at the edge.
        assert_eq!(
            a.arrived(id.clone()).unwrap().as_deref(),
            Some("buy screws")
        );
        assert_eq!(
            a.arrived(id).unwrap(),
            None,
            "a phone re-acquiring its position is not a second visit"
        );
    } else {
        assert!(
            watching.is_err(),
            "a machine that does not move cannot watch a place: it would never \
             fire, or fire once and never stop"
        );
    }

    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn a_recording_is_captured_with_no_words_and_read_back() {
    // §2.2: the phone's job ends at "the audio exists". No transcription here,
    // and the note's permanent name is not derived from one — §4.3 forbids it,
    // because machine output changes and a name must not.
    let root = scratch("voice");
    let a = Archive::open(root.clone()).unwrap();

    let recording = vec![0x33u8; 5000];
    let slug = a
        .capture_voice(recording.clone(), "audio/ogg".into(), Some(9310))
        .unwrap();

    let note = a.note(slug.clone()).unwrap();
    assert_eq!(note.slug, slug);
    assert!(
        note.text.as_deref().unwrap_or("").is_empty(),
        "a recording has no words yet, and inventing some would be a guess \
         written into the log"
    );
    // Named from its date, because there are no words to name it from — and
    // that date is UTC, which is not always the same day as here. A name has
    // to be identical on every replica for ever, so it cannot be computed
    // through a timezone database that devices carry their own versions of.
    // Which day the note is *filed* under is a separate question, answered in
    // the note's own zone by `NoteSummary::day`.
    let named_for = &slug[..10];
    assert!(
        named_for.starts_with("20") && named_for.matches('-').count() == 2,
        "a textless capture is named from its date: {slug}"
    );
    assert!(
        a.note(slug.clone()).unwrap().day.len() == 10,
        "and it is still filed under a day computed where it happened"
    );

    // The note knows what it refers to, so a surface can offer to play it.
    let held = a.recording_of(slug.clone()).unwrap().expect("it has one");
    assert_eq!(held.media_type, "audio/ogg");
    assert_eq!(held.bytes, recording.len() as u64);
    assert_eq!(held.duration_ms, Some(9310));
    assert!(held.here, "this device recorded it, so it holds it");
    assert_eq!(
        a.recording(held.name.clone()).unwrap().as_deref(),
        Some(&recording[..]),
        "and the bytes come back byte for byte"
    );

    // The bytes are addressed by their content, so the same recording captured
    // twice is one file. Nothing dedupes; the naming makes it impossible.
    let again = a
        .capture_voice(recording.clone(), "audio/ogg".into(), Some(9310))
        .unwrap();
    assert_ne!(again, slug, "two captures are two notes");
    assert_eq!(
        a.recording_of(again).unwrap().map(|r| r.name),
        Some(held.name),
        "pointing at one recording, because the content decides the name"
    );

    // A note with no recording says so, rather than failing.
    let typed = match a.capture("an ordinary typed note".into()).unwrap() {
        Captured::Stored { slug } => slug,
        Captured::Spooled => panic!("spooled"),
    };
    assert!(a.recording_of(typed).unwrap().is_none());

    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn each_capture_surface_reports_what_it_cost() {
    // §2.1 argues capture must cost almost nothing at the moment a thought
    // arrives, and 4.5 asks the phone to answer that rather than assert it.
    // Per surface, because they differ in what they must do first — a share
    // sheet arrives with the content chosen, a recording has to be encoded and
    // hashed — and one number over all of them averages away the only useful
    // fact, which is *which* gesture is slow.
    let root = scratch("cost");
    let a = Archive::open(root.clone()).unwrap();

    assert!(
        a.costs().unwrap().is_empty(),
        "nothing is claimed before anything is measured"
    );

    a.took("tile".into(), 42).unwrap();
    a.took("tile".into(), 58).unwrap();
    a.took("voice".into(), 900).unwrap();

    let costs = a.costs().unwrap();
    assert_eq!(costs.len(), 2, "one line per surface: {costs:?}");
    assert!(
        costs.iter().any(|c| c.starts_with("tile: 2 shown")),
        "{costs:?}"
    );
    assert!(
        costs.iter().any(|c| c.starts_with("voice: 1 shown")),
        "{costs:?}"
    );

    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn two_things_at_the_same_time_are_drawn_side_by_side() {
    // §4.4's column packing, through the surface a phone calls. Two meetings
    // at nine are two meetings; a day that put them in one column would have a
    // surface draw them on top of each other, and the one underneath is the
    // one somebody misses.
    //
    // Promoted by hand rather than through the recogniser, which is a separate
    // question with its own tests. This is about what happens to occurrences
    // once they exist.
    let root = scratch("side-by-side");
    let a = Archive::open(root.clone()).unwrap();
    let paths = capture_store::paths::StorePaths::under(std::path::Path::new(&root));

    // Daily, so there is one today whatever day this runs on — an expansion
    // horizon is measured from now, and a date picked in advance falls outside
    // it eventually.
    let now = jiff::Timestamp::now().as_millisecond();
    let daily = |at: i64| {
        capture_core::body::Promoted::Series(capture_core::body::SeriesFields {
            when: capture_core::body::When::Timed {
                start_ms: at,
                end_ms: Some(at + 60 * 60 * 1000),
                tz: "UTC".into(),
            },
            rule: capture_core::body::RuleSpec {
                freq: 1,
                interval: 1,
                by_day: 0b0111_1111,
                by_month_day: Vec::new(),
                count: None,
                until: None,
            },
            carries_forward: false,
            split_from: None,
        })
    };

    for (what, at) in [
        ("standup", now),
        ("design review", now),
        ("lunch", now + 5 * 60 * 60 * 1000),
    ] {
        let slug = match a.capture(what.into()).unwrap() {
            Captured::Stored { slug } => slug,
            Captured::Spooled => panic!("spooled"),
        };
        capture_store::promote::correct(&paths, &slug, daily(at)).unwrap();
    }

    let today = a.today();
    let on_axis: Vec<_> = a
        .agenda(today.clone())
        .unwrap()
        .into_iter()
        .filter(|e| !e.all_day && !e.due)
        .collect();

    // Grouped by the occurrence's own start rather than compared against the
    // moment the series was written: expansion places an occurrence where the
    // rule says, which is near that moment and not equal to it.
    let earliest = on_axis
        .iter()
        .map(|e| e.start_ms)
        .min()
        .expect("a day with things on it");
    let at_once: Vec<_> = on_axis.iter().filter(|e| e.start_ms == earliest).collect();
    assert_eq!(
        at_once.len(),
        2,
        "two things at once on {today}: {on_axis:?}"
    );
    assert_eq!(
        at_once[0].columns, 2,
        "two things at once are two columns wide"
    );
    assert_ne!(
        at_once[0].column, at_once[1].column,
        "and not in the same one, or they are drawn on top of each other"
    );

    // The cluster decides the width, not the day: one overlap in the morning
    // must not squeeze an unrelated lunch into half the page.
    if let Some(alone) = on_axis.iter().find(|e| e.start_ms != earliest) {
        assert_eq!(alone.columns, 1, "a lonely entry has the width to itself");
        assert_eq!(alone.column, 0);
    }

    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn overlapping_things_on_a_day_get_columns_of_their_own() {
    // §4.4. Two meetings at the same time are two meetings, and a surface that
    // drew them in one column would draw them on top of each other. The
    // packing is the same code the desktop runs — two implementations would be
    // two answers to "do these overlap", visible only as two devices
    // disagreeing about what Tuesday looks like.
    let root = scratch("packing");
    let a = Archive::open(root.clone()).unwrap();

    let day = a.today();
    let entries = a.agenda(day.clone()).unwrap();

    // An empty day still answers, and answers with a width of one rather than
    // zero: a surface dividing by `columns` should not have to know that a day
    // with nothing on it is a special case.
    for e in &entries {
        assert!(e.columns >= 1, "a cluster is at least one column wide");
        assert!(e.column < e.columns, "a column sits inside its cluster");
    }

    // All-day entries and due tasks are left off the hour axis entirely, which
    // is why they keep the defaults rather than being packed against hours
    // they were never given.
    for e in entries.iter().filter(|e| e.all_day || e.due) {
        assert_eq!(e.column, 0);
        assert_eq!(e.columns, 1);
    }

    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn a_capture_that_names_a_time_reaches_the_agenda() {
    // §3.1 puts parsing after the commit, so a capture never waits for it —
    // and something has to run the sweep afterwards or a note that plainly
    // says "9am" is never promoted, never expanded, and never appears on any
    // calendar. On the desktop the next command that writes does it. A phone
    // that only captures and reads never writes anything else, so `settle` is
    // the one call that has to.
    let root = scratch("promoted");
    let a = Archive::open(root.clone()).unwrap();

    a.capture("standup at 9am".into()).unwrap();

    // What a phone does when it comes forward.
    a.settle().unwrap();

    let today = a.today();
    let on_day = a.agenda(today.clone()).unwrap();
    assert!(
        !on_day.is_empty(),
        "nothing reached the agenda for {today}: a note saying 9am is not on \
         any calendar, on any device, until something else happens to write"
    );

    // And it is the one-off that arrived, which is the case that was missing:
    // a single-instance event lives in `promotions` and gains an occurrence
    // row only once somebody writes an exception against it, so a query over
    // expansions alone finds nothing.
    assert_eq!(on_day.len(), 1);
    assert!(!on_day[0].all_day, "a moment, not a floating date");

    std::fs::remove_dir_all(&root).ok();
}

/// The agenda asks about a *local* day, so it must read one.
///
/// This is a regression guard for a bug that hid in plain sight for a
/// milestone: `today()` answered in the system zone while `agenda()` turned
/// that day into a UTC window. The two agreed for most of the day and stopped
/// agreeing every evening west of Greenwich — 19:00 in Chicago is already
/// tomorrow in UTC — so the phone's agenda went empty for the last hours of
/// each day and filled itself back in overnight.
///
/// Asserted by the clock rather than by inspection, because the failure only
/// existed at certain hours: a test that ran at noon saw nothing wrong, which
/// is exactly how it survived being written, reviewed and shipped.
#[test]
fn the_agenda_reads_the_day_it_was_asked_about() {
    let root = scratch("agenda-zone");
    let a = Archive::open(root.clone()).unwrap();
    let paths = capture_store::paths::StorePaths::under(std::path::Path::new(&root));

    let slug = match a.capture("something happening now".into()).unwrap() {
        Captured::Stored { slug } => slug,
        Captured::Spooled => panic!("spooled"),
    };

    // Now, in the zone the device is in — which is what somebody capturing
    // "meeting at 3" means and what `today()` is about to answer with.
    let zone = jiff::tz::TimeZone::system();
    let now = jiff::Timestamp::now().as_millisecond();
    capture_store::promote::correct(
        &paths,
        &slug,
        capture_core::body::Promoted::Event(capture_core::body::EventFields {
            when: capture_core::body::When::Timed {
                start_ms: now,
                end_ms: Some(now + 60 * 60 * 1000),
                tz: zone.iana_name().unwrap_or("UTC").into(),
            },
            location: None,
            attendees: Vec::new(),
        }),
    )
    .unwrap();

    let today = a.today();
    let on_today = a.agenda(today.clone()).unwrap();
    assert!(
        on_today.iter().any(|e| !e.all_day && !e.due),
        "something happening right now is on today's agenda, whatever hour this \
         runs at — today={today}, got {on_today:?}",
    );
}

/// Two queries, one answer.
///
/// The timeline and the note page are separate reads, and the argument for
/// keeping a promotion's rendering in the projection rather than composing it
/// per surface is that two readings of one note must not disagree. They would
/// the first time somebody fixed one call site and forgot the other, and the
/// disagreement would be invisible — each screen looks correct on its own.
#[test]
fn a_note_reads_the_same_through_the_timeline_and_on_its_own() {
    let root = scratch("one-answer");
    let a = Archive::open(root.clone()).unwrap();

    for text in [
        "an ordinary thought",
        "#groceries: milk, eggs, bread",
        "call the dentist tomorrow",
    ] {
        match a.capture(text.into()).unwrap() {
            Captured::Stored { .. } => {}
            Captured::Spooled => panic!("spooled"),
        }
    }
    a.settle().unwrap();

    for row in a.timeline(50).unwrap() {
        let alone = a.note(row.slug.clone()).unwrap();
        assert_eq!(row.kind, alone.kind, "kind differs for {}", row.slug);
        assert_eq!(
            row.descriptor, alone.descriptor,
            "descriptor differs for {}",
            row.slug,
        );
        assert_eq!(
            row.items.iter().map(|i| &i.text).collect::<Vec<_>>(),
            alone.items.iter().map(|i| &i.text).collect::<Vec<_>>(),
            "items differ for {}",
            row.slug,
        );
        assert_eq!(
            row.list_tag, alone.list_tag,
            "list tag differs for {}",
            row.slug
        );
    }
}

/// An ordinary note is not dressed up.
#[test]
fn a_capture_with_no_promotion_gets_no_invented_rendering() {
    let root = scratch("no-invention");
    let a = Archive::open(root.clone()).unwrap();

    match a.capture("an ordinary thought".into()).unwrap() {
        Captured::Stored { .. } => {}
        Captured::Spooled => panic!("spooled"),
    }
    a.settle().unwrap();

    let row = &a.timeline(10).unwrap()[0];
    assert_eq!(row.text.as_deref(), Some("an ordinary thought"));
    assert!(
        row.descriptor.is_empty(),
        "a note's content is its text; §3.1.3 forbids adding facts it did not \
         contain, and \"untitled\" is one: {:?}",
        row.descriptor,
    );
    assert!(row.items.is_empty(), "and it names nothing");
}

/// A list capture answers its own items, not the tag's.
#[test]
fn a_row_shows_its_own_items_and_not_the_tags() {
    let root = scratch("own-items");
    let a = Archive::open(root.clone()).unwrap();

    for text in ["#groceries: milk, eggs", "#groceries: bread"] {
        match a.capture(text.into()).unwrap() {
            Captured::Stored { .. } => {}
            Captured::Spooled => panic!("spooled"),
        }
    }
    a.settle().unwrap();

    for row in a.timeline(10).unwrap() {
        let items: Vec<_> = row.items.iter().map(|i| i.text.as_str()).collect();
        match row.text.as_deref() {
            Some("#groceries: milk, eggs") => assert_eq!(items, ["milk", "eggs"]),
            Some("#groceries: bread") => assert_eq!(items, ["bread"]),
            other => panic!("unexpected row {other:?}"),
        }
    }

    // The tag-wide view still answers the wider question.
    assert_eq!(a.list("groceries".into()).unwrap().len(), 3);
}

/// An archive whose projection predates this build rebuilds itself on open.
///
/// This was found on a real phone, as a SQL error drawn across the timeline:
/// `no such column: li.done`. The schema had gained a column, `CREATE TABLE IF
/// NOT EXISTS` does nothing to a table that already exists — which
/// `projection.rs` warns about twice — and the version bump that was supposed
/// to force a rebuild was only acted on by `Session::open`. The phone's
/// reading path opens the current generation directly and never opens a
/// session, so a cold launch after an upgrade read the old shape.
///
/// Simulated by writing an older schema version into the projection's own
/// metadata, which is exactly the state an upgrade leaves behind.
#[test]
fn a_projection_from_an_older_build_is_rebuilt_before_it_is_read() {
    let root = scratch("stale-schema");

    let slugs: Vec<String> = {
        let a = Archive::open(root.clone()).unwrap();
        ["an ordinary thought", "#groceries: milk, eggs"]
            .into_iter()
            .map(|t| match a.capture(t.into()).unwrap() {
                Captured::Stored { slug } => slug,
                Captured::Spooled => panic!("spooled"),
            })
            .collect()
    };

    // Age it. The generation on disk is now claiming a shape this build does
    // not have.
    {
        let paths = capture_store::paths::StorePaths::under(std::path::Path::new(&root));
        let g = capture_store::ops::Ops::open_read(&paths.ops)
            .unwrap()
            .generation()
            .unwrap();
        let conn =
            rusqlite::Connection::open(capture_store::projection::generation_path(&paths.state, g))
                .unwrap();
        // The column has to actually go. Claiming an older version while the
        // table still has every column is not the state an upgrade leaves
        // behind — and a test written that way passes whether or not anything
        // rebuilds, which is worse than no test.
        conn.execute("ALTER TABLE list_items DROP COLUMN done", [])
            .unwrap();
        conn.execute("UPDATE meta SET v = '6' WHERE k = 'schema'", [])
            .unwrap();
    }

    // Opening is the whole test: a read that throws has already failed the
    // person holding the phone, whatever the next line does.
    let a = Archive::open(root.clone()).unwrap();
    let rows = a.timeline(10).expect("the timeline reads after an upgrade");
    assert_eq!(rows.len(), slugs.len(), "and holds what was captured");

    let list = a.list("groceries".into()).expect("and so does a list");
    assert_eq!(
        list.iter().map(|l| l.text.as_str()).collect::<Vec<_>>(),
        ["milk", "eggs"],
    );
}

/// What a timeline read costs now that every row carries its rendering.
///
/// Ignored, and run by hand: a timing assertion in CI measures whatever else
/// the machine was doing. The number is the point, not a threshold — §2.1
/// makes what capture costs the thing worth knowing, and reading is the other
/// half of the same budget.
///
/// `cargo test -p capture-ffi --test surface -- --ignored --nocapture`
#[test]
#[ignore = "a measurement, not an assertion"]
#[allow(
    clippy::disallowed_methods,
    clippy::disallowed_types,
    reason = "measuring how long a read takes is what a monotonic clock is for"
)]
fn what_a_timeline_read_costs() {
    let root = scratch("cost");
    let a = Archive::open(root.clone()).unwrap();

    // A mix, because the cost is per row and differs by what a row holds: a
    // list row asks the projection for its items and an ordinary note does
    // not.
    for i in 0..100 {
        let text = match i % 4 {
            0 => format!("#groceries: milk {i}, eggs, bread, coffee"),
            1 => format!("call the dentist tomorrow about {i}"),
            2 => format!("standup every weekday at 9 — {i}"),
            _ => format!("an ordinary thought number {i}"),
        };
        match a.capture(text).unwrap() {
            Captured::Stored { .. } => {}
            Captured::Spooled => panic!("spooled"),
        }
    }
    a.settle().unwrap();

    // Warm, so this measures the read and not the first open of the database.
    let _ = a.timeline(100).unwrap();

    let runs = 20;
    let began = std::time::Instant::now();
    for _ in 0..runs {
        let rows = a.timeline(100).unwrap();
        assert_eq!(rows.len(), 100);
    }
    let each = began.elapsed() / runs;

    let rows = a.timeline(100).unwrap();
    let lists = rows.iter().filter(|r| !r.items.is_empty()).count();
    let items: usize = rows.iter().map(|r| r.items.len()).sum();

    println!("\n  100 rows, {lists} of them lists holding {items} items: {each:?} per read\n");
}

/// Retiring a device, and the guards that stop it being a way to lose one.
///
/// Reachable from the phone because the phone may be the only device left.
/// Retiring a lost *phone* from the desktop was always possible; a lost
/// desktop left the phone holding the archive and unable to close anything,
/// which is the half of §5.3's remedy that was never reachable from the
/// surface most likely to survive.
#[test]
fn retiring_the_only_device_is_refused() {
    let root = scratch("retire-alone");
    let a = Archive::open(root.clone()).unwrap();

    let me = a.device_key().unwrap();

    // A roster with nobody in it can never admit anyone again, and there is no
    // way back: readmission needs a signature from a member, and there would
    // be none. So this is refused rather than confirmed.
    let out = a.retire(me.clone());
    assert!(
        out.is_err(),
        "retiring the only device would strand the archive forever: {out:?}",
    );
}

#[test]
fn a_key_that_names_nobody_is_not_guessed_at() {
    let root = scratch("retire-absent");
    let a = Archive::open(root.clone()).unwrap();

    let out = a.retire("ffffffffffff".into());
    assert!(out.is_err(), "no device matches, so nothing is retired");
}

/// A device cannot vouch for its own way back in.
///
/// §5.3 needs two signatures precisely because any member may retire any
/// member. One device signing twice is one device's say-so written twice, and
/// it would reduce the bar back to the single compromised device that caused
/// the lockout.
#[test]
fn a_device_cannot_cosign_for_itself() {
    let root = scratch("cosign-self");
    let a = Archive::open(root.clone()).unwrap();

    let request = a.readmission_request().unwrap();
    let out = a.cosign(request);
    assert!(out.is_err(), "self-vouching is refused: {out:?}");
}

#[test]
fn a_readmission_request_names_this_device() {
    let root = scratch("request-shape");
    let a = Archive::open(root.clone()).unwrap();

    let request = a.readmission_request().unwrap();
    let (key, incarnation) = request.split_once('.').expect("key.incarnation");
    assert_eq!(key.len(), 64, "a full signing key, hex");
    assert_eq!(
        incarnation.len(),
        32,
        "and the incarnation it authors under"
    );
    assert!(
        key.starts_with(&a.device_key().unwrap()),
        "and it is this device: {request}",
    );

    // Not a secret. It says which device is asking and nothing else, and a
    // signature over it is useless without the second one from the device it
    // names.
    assert!(request.chars().all(|c| c.is_ascii_hexdigit() || c == '.'));
}

#[test]
fn nonsense_is_not_a_readmission_request() {
    let root = scratch("cosign-junk");
    let a = Archive::open(root.clone()).unwrap();

    for junk in ["", "hello", "zz.zz", "abc", "abcd."] {
        assert!(
            a.cosign(junk.into()).is_err(),
            "{junk:?} should not be read as a request",
        );
    }
}

// ------------------------------------------------------------- kinds -------
//
// Saying what a capture is, and asking what is outstanding — both through the
// binding rather than the store, because the generator, JNA and the ABI are
// exactly the layer the Rust suite does not cover. A pinned flag lost in
// translation looks correct on both sides of it.

/// Run the sweep the way opening a session does, against an archive's store.
fn sweep(root: &str) -> usize {
    let paths = capture_store::paths::StorePaths::under(std::path::Path::new(root));
    let mut s = capture_store::session::Session::open(paths).unwrap();
    capture_store::promote::sweep(&mut s).unwrap()
}

fn kind_of(a: &Archive, slug: &str) -> String {
    a.note(slug.to_string()).unwrap().kind
}

#[test]
fn a_correction_through_the_binding_survives_a_later_sweep() {
    // The recognizer would call this an event — a day and a clock time, well
    // over the floor. A person saying it is a task has to win, and has to keep
    // winning however many times the grammar is re-run.
    let root = scratch("correction-sticks");
    let a = Archive::open(root.clone()).unwrap();

    let Captured::Stored { slug } = a.capture("dentist thursday 2pm".into()).unwrap() else {
        panic!("it stored");
    };
    sweep(&root);
    assert_eq!(kind_of(&a, &slug), "event", "the recognizer's own answer");

    a.as_task(slug.clone(), None).unwrap();
    assert_eq!(kind_of(&a, &slug), "task");

    // Twice, because once could be a sweep that simply had nothing to do.
    sweep(&root);
    sweep(&root);
    assert_eq!(
        kind_of(&a, &slug),
        "task",
        "a reparse displaced somebody's correction"
    );

    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn saying_what_something_is_does_not_touch_what_was_captured() {
    // Promotion annotates; it never rewrites (§3.1.3). This is the invariant a
    // reclassify surface is most likely to break, so it is checked on every
    // path that writes a kind.
    let root = scratch("text-untouched");
    let a = Archive::open(root.clone()).unwrap();

    let text = "send sarah the migration doc";
    let Captured::Stored { slug } = a.capture(text.into()).unwrap() else {
        panic!("it stored");
    };

    for step in ["accept", "as_task", "set_kind_note", "as_task_due"] {
        match step {
            // Every path that writes a kind, including the one that takes the
            // parser's own reading — a suggestion applied is still a promotion,
            // and a promotion annotates rather than rewrites.
            "accept" => {
                a.set_kind(slug.clone(), capture_ffi::NoteKind::Note)
                    .unwrap();
                a.accept(slug.clone()).unwrap();
            }
            "as_task" => a.as_task(slug.clone(), None).unwrap(),
            "set_kind_note" => a
                .set_kind(slug.clone(), capture_ffi::NoteKind::Note)
                .unwrap(),
            _ => a.as_task(slug.clone(), Some("2026-09-20".into())).unwrap(),
        }
        assert_eq!(
            a.note(slug.clone()).unwrap().text.as_deref(),
            Some(text),
            "the captured text changed after {step}"
        );
    }

    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn a_bare_task_verb_is_only_a_task_once_somebody_says_so() {
    // The floor refusing to guess, and the correction that makes the refusal
    // survivable. Lowering the floor would remove the need for the second half
    // by making the first half wrong more often.
    let root = scratch("verb-alone");
    let a = Archive::open(root.clone()).unwrap();

    let Captured::Stored { slug } = a.capture("send sarah the migration doc".into()).unwrap()
    else {
        panic!("it stored");
    };
    sweep(&root);
    assert_eq!(kind_of(&a, &slug), "note", "a verb alone was promoted");
    assert!(a.tasks().unwrap().is_empty(), "a note reached the tasks");

    a.as_task(slug.clone(), None).unwrap();
    let tasks = a.tasks().unwrap();
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].slug, slug);
    assert_eq!(tasks[0].band, capture_ffi::TaskBand::Undated);
    assert!(tasks[0].due.is_none());

    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn a_finished_task_is_not_outstanding_and_a_demoted_one_is_not_a_task() {
    let root = scratch("leaves-the-bands");
    let a = Archive::open(root.clone()).unwrap();

    let Captured::Stored { slug } = a.capture("call the dentist".into()).unwrap() else {
        panic!("it stored");
    };
    a.as_task(slug.clone(), None).unwrap();
    assert_eq!(a.tasks().unwrap().len(), 1);

    a.set_done(slug.clone(), true).unwrap();
    assert!(
        a.tasks().unwrap().is_empty(),
        "a finished task stayed outstanding"
    );

    // Undone, then demoted. Both directions exist because a thing wrongly
    // promoted is as ordinary as a thing wrongly left alone.
    a.set_done(slug.clone(), false).unwrap();
    assert_eq!(
        a.tasks().unwrap().len(),
        1,
        "un-ticking did not bring it back"
    );

    a.set_kind(slug.clone(), capture_ffi::NoteKind::Note)
        .unwrap();
    assert_eq!(kind_of(&a, &slug), "note");
    assert!(a.tasks().unwrap().is_empty(), "a plain note is not a task");

    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn a_due_date_crosses_as_a_floating_day() {
    // Never an instant. "Due Friday" is not 00:00 UTC on Friday (§3.6), and the
    // string that comes back is the normalised day rather than an epoch.
    let root = scratch("floating-due");
    let a = Archive::open(root.clone()).unwrap();

    let Captured::Stored { slug } = a.capture("file the accounts".into()).unwrap() else {
        panic!("it stored");
    };
    // Far enough out that the test does not start failing by being old.
    // `tasks()` bands against the real clock, deliberately: what day it is has
    // one implementation and the caller does not get to supply its own.
    a.as_task(slug.clone(), Some("2099-01-01".into())).unwrap();

    let tasks = a.tasks().unwrap();
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].due.as_deref(), Some("2099-01-01"));
    assert_eq!(tasks[0].band, capture_ffi::TaskBand::Due);

    // And a day already gone is overdue rather than merely due.
    a.as_task(slug.clone(), Some("2020-01-01".into())).unwrap();
    assert_eq!(a.tasks().unwrap()[0].band, capture_ffi::TaskBand::Overdue);
    a.as_task(slug.clone(), Some("2099-01-01".into())).unwrap();

    // A day is padded or it is not a day. `FloatingDate::parse` is a
    // fixed-width read, so `2026-6-1` is refused rather than normalised —
    // which is worth pinning down, because the comment above `as_task` claims
    // the opposite and a caller believing it would build the string wrong.
    for not_a_date in ["2026-6-1", "the first of june", "", "2026-13-01"] {
        assert!(
            a.as_task(slug.clone(), Some(not_a_date.into())).is_err(),
            "{not_a_date:?} was accepted as a date"
        );
    }

    // Refused means refused: the task still carries the day it was given.
    assert_eq!(
        a.tasks().unwrap()[0].due.as_deref(),
        Some("2099-01-01"),
        "a rejected date disturbed the one already set"
    );

    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn a_suggestion_is_offered_and_can_be_taken() {
    // The floor's other half. "send sarah the migration doc" scores 40 against
    // a floor of 75, so the recognizer will not act on it — and the person who
    // typed it is better placed than the parser to say it was a task.
    let root = scratch("accept-suggestion");
    let a = Archive::open(root.clone()).unwrap();

    let Captured::Stored { slug } = a.capture("send sarah the migration doc".into()).unwrap()
    else {
        panic!("it stored");
    };
    sweep(&root);

    assert_eq!(
        kind_of(&a, &slug),
        "note",
        "it was promoted without being asked"
    );
    let offered = a.suggestion(slug.clone()).unwrap();
    assert!(
        offered.as_deref().unwrap_or_default().contains("a task"),
        "expected a task suggestion, got {offered:?}"
    );

    assert_eq!(a.accept(slug.clone()).unwrap(), "task");
    assert_eq!(kind_of(&a, &slug), "task");

    // Pinned, so the grammar does not take it back.
    sweep(&root);
    sweep(&root);
    assert_eq!(
        kind_of(&a, &slug),
        "task",
        "a sweep undid an accepted reading"
    );

    // And it is outstanding, which is the point of having accepted it.
    let tasks = a.tasks().unwrap();
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].band, capture_ffi::TaskBand::Undated);

    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn there_is_nothing_to_accept_on_a_confident_reading_or_a_plain_note() {
    let root = scratch("nothing-to-accept");
    let a = Archive::open(root.clone()).unwrap();

    // Well over the floor: already applied, so it was never a suggestion.
    let Captured::Stored { slug: confident } = a.capture("dentist thursday 2pm".into()).unwrap()
    else {
        panic!("it stored");
    };
    sweep(&root);
    assert_eq!(kind_of(&a, &confident), "event");
    assert!(a.suggestion(confident.clone()).unwrap().is_none());
    assert!(
        a.accept(confident.clone()).is_err(),
        "accepting an applied reading should refuse"
    );

    // Nothing temporal in it at all.
    let Captured::Stored { slug: prose } =
        a.capture("the sky was a bright cold blue".into()).unwrap()
    else {
        panic!("it stored");
    };
    sweep(&root);
    assert!(a.suggestion(prose.clone()).unwrap().is_none());
    assert!(
        a.accept(prose.clone()).is_err(),
        "there was nothing to take"
    );
    assert_eq!(kind_of(&a, &prose), "note");

    std::fs::remove_dir_all(&root).ok();
}

/// What the task read costs at a few hundred open tasks.
///
/// A measurement rather than an assertion, for the reason the timeline's
/// version gives: a threshold in CI measures whatever else the machine was
/// doing. The claim worth checking is the shape — one indexed query against
/// `promotions`, banded in the store, rather than a query per row.
///
/// `cargo test -p capture-ffi --test surface -- --ignored --nocapture`
#[test]
#[ignore = "a measurement, not an assertion"]
#[allow(
    clippy::disallowed_methods,
    clippy::disallowed_types,
    reason = "measuring how long a read takes is what a monotonic clock is for"
)]
fn what_the_task_read_costs() {
    let root = scratch("task-cost");
    let a = Archive::open(root.clone()).unwrap();

    // A spread across all three bands, because banding is the work this read
    // does that `promoted("task", n)` does not.
    let mut slugs = Vec::new();
    for i in 0..300 {
        let Captured::Stored { slug } = a.capture(format!("a thing to do, number {i}")).unwrap()
        else {
            panic!("spooled");
        };
        slugs.push(slug);
    }
    a.settle().unwrap();

    for (i, slug) in slugs.iter().enumerate() {
        let due = match i % 3 {
            0 => None,
            1 => Some(String::from("2020-01-01")),
            _ => Some(String::from("2099-01-01")),
        };
        a.as_task(slug.clone(), due).unwrap();
    }
    a.settle().unwrap();

    // Warm, so this is the read rather than the first open of the database.
    let _ = a.tasks().unwrap();

    let runs = 20;
    let began = std::time::Instant::now();
    for _ in 0..runs {
        assert_eq!(a.tasks().unwrap().len(), 300);
    }
    let each = began.elapsed() / runs;

    let rows = a.tasks().unwrap();
    let overdue = rows
        .iter()
        .filter(|r| r.band == capture_ffi::TaskBand::Overdue)
        .count();
    let undated = rows
        .iter()
        .filter(|r| r.band == capture_ffi::TaskBand::Undated)
        .count();

    println!("\n  300 open tasks ({overdue} overdue, {undated} undated): {each:?} per read\n");

    std::fs::remove_dir_all(&root).ok();
}

// ---------------------------------------------------------- entities -------
//
// Through the binding rather than the store, because the generator, JNA and the
// ABI are precisely what the Rust suite does not cover — and identity crossing
// that boundary wrong looks correct on both sides of it.

#[test]
fn case_does_not_make_two_people_through_the_binding() {
    let root = scratch("ent-case");
    let a = Archive::open(root.clone()).unwrap();

    a.capture("ask @sarah about the invoice".into()).unwrap();
    a.capture("@Sarah replied".into()).unwrap();

    let people = a.people().unwrap();
    assert_eq!(people.len(), 1, "case forked a person");
    assert_eq!(people[0].notes.len(), 2);
    assert_eq!(people[0].display, "sarah", "the folded key was shown");

    // Either spelling asks the same question.
    assert_eq!(
        a.entity("@SARAH".into()).unwrap().map(|e| e.notes.len()),
        Some(2)
    );

    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn a_rename_does_not_fork_the_page_through_the_binding() {
    let root = scratch("ent-rename");
    let a = Archive::open(root.clone()).unwrap();

    a.capture("ask @sarah about the invoice".into()).unwrap();
    a.capture("@sarah sent it".into()).unwrap();

    a.rename_entity("@sarah".into(), "Sarah Chen".into())
        .unwrap();

    let page = a.entity("@sarah".into()).unwrap().expect("still there");
    assert_eq!(page.display, "Sarah Chen");
    assert_eq!(page.notes.len(), 2, "a rename forked the page");

    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn a_merge_is_followed_through_the_binding() {
    let root = scratch("ent-merge");
    let a = Archive::open(root.clone()).unwrap();

    a.capture("ask @sarah about the invoice".into()).unwrap();
    a.capture("@sarah-chen sent the contract".into()).unwrap();
    assert_eq!(a.people().unwrap().len(), 2, "two spellings start as two");

    a.merge_entities("@sarah-chen".into(), "@sarah".into())
        .unwrap();

    let people = a.people().unwrap();
    assert_eq!(people.len(), 1, "a merged-away entity is still listed");
    assert_eq!(people[0].notes.len(), 2);
    assert_eq!(people[0].names.len(), 2, "both spellings should reach it");

    // Either name arrives at the survivor.
    for name in ["@sarah", "@sarah-chen"] {
        assert_eq!(
            a.entity(name.into()).unwrap().map(|e| e.notes.len()),
            Some(2),
            "{name} did not reach the merged page"
        );
    }

    // And merging across kinds is refused rather than quietly joining a person
    // to the project named after them.
    a.capture("the ^sarah-chen migration".into()).unwrap();
    assert!(
        a.merge_entities("@sarah".into(), "^sarah-chen".into())
            .is_err(),
        "a person was merged into a project"
    );

    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn a_bind_does_not_retype_the_note_through_the_binding() {
    let root = scratch("ent-bind");
    let a = Archive::open(root.clone()).unwrap();

    let Captured::Stored { slug } = a
        .capture("the ^acme migration, everything about it".into())
        .unwrap()
    else {
        panic!("it stored");
    };
    a.capture("^acme slipped a week".into()).unwrap();

    // A task deliberately: §4.3 says binding must not change what a note is.
    a.as_task(slug.clone(), None).unwrap();
    a.bind_backing("^acme".into(), Some(slug.clone())).unwrap();

    let page = a.entity("^acme".into()).unwrap().unwrap();
    assert_eq!(
        page.backing.as_ref().map(|b| b.slug.clone()),
        Some(slug.clone())
    );
    assert_eq!(page.notes.len(), 1, "the backing note appeared twice");
    assert_eq!(kind_of(&a, &slug), "task", "binding retyped the note");
    assert_eq!(a.tasks().unwrap().len(), 1, "it stopped being outstanding");

    // Unbinding puts it back among the notes.
    a.bind_backing("^acme".into(), None).unwrap();
    let page = a.entity("^acme".into()).unwrap().unwrap();
    assert!(page.backing.is_none(), "unbinding was skipped");
    assert_eq!(page.notes.len(), 2);

    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn a_name_nobody_wrote_is_not_an_error() {
    let root = scratch("ent-nobody");
    let a = Archive::open(root.clone()).unwrap();
    a.capture("an ordinary thought naming nobody".into())
        .unwrap();

    assert!(a.entity("@nobody".into()).unwrap().is_none());
    assert!(a.people().unwrap().is_empty());
    assert!(a.projects().unwrap().is_empty());

    // A mention that is not one is refused, rather than guessed at.
    assert!(a.entity("sarah".into()).is_err());
    assert!(a.entity("@".into()).is_err());

    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn note_text_splits_into_runs_and_mentions() {
    let root = scratch("pieces");
    let a = Archive::open(root.clone()).unwrap();

    let pieces = a.pieces("ask @sarah about ^acme today".into());
    let mentions: Vec<String> = pieces.iter().filter_map(|p| p.mention.clone()).collect();
    assert_eq!(mentions, vec!["@sarah", "^acme"]);

    // Reassembles to exactly what was written. A splitter that dropped or
    // duplicated a character would show the reader something they did not type.
    let rejoined: String = pieces.iter().map(|p| p.text.as_str()).collect();
    assert_eq!(rejoined, "ask @sarah about ^acme today");

    // An address is not a person, and the guard is structural rather than a
    // special case: a sigil has to open a token.
    let pieces = a.pieces("mail sarah@example.com".into());
    assert!(pieces.iter().all(|p| p.mention.is_none()));

    // Text with nothing in it comes back as one ordinary run.
    let pieces = a.pieces("an ordinary thought".into());
    assert_eq!(pieces.len(), 1);
    assert!(pieces[0].mention.is_none());

    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn merging_accepts_the_other_name_written_bare() {
    // Found by driving a phone: the page being looked at already says it is a
    // person, so demanding the sigil again asks somebody to restate what is on
    // screen — and typing it without was the first thing that happened.
    let root = scratch("ent-bare");
    let a = Archive::open(root.clone()).unwrap();

    a.capture("ask @sarah about the invoice".into()).unwrap();
    a.capture("@sarah-chen sent the contract".into()).unwrap();

    a.merge_entities("sarah-chen".into(), "@sarah".into())
        .unwrap();

    assert_eq!(a.people().unwrap().len(), 1, "the bare name did not merge");
    assert_eq!(
        a.entity("@sarah-chen".into()).unwrap().unwrap().notes.len(),
        2
    );

    // The one being merged *into* still needs its sigil: nothing else on that
    // side says what kind it is.
    assert!(a.merge_entities("@a".into(), "b".into()).is_err());

    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn two_identical_items_are_tickable_through_the_binding() {
    // Untickable before this. `tick` matched by words, found two, and refused —
    // and a phone showing the list always knew which row a thumb landed on.
    let root = scratch("tick-at");
    let a = Archive::open(root.clone()).unwrap();

    let Captured::Stored { slug } = a.capture("#shop: milk, milk, bread".into()).unwrap() else {
        panic!("it stored");
    };

    assert!(matches!(
        a.tick(("shop").into(), "milk".into(), true).unwrap(),
        capture_ffi::Ticked::Ambiguous { count: 2 }
    ));

    a.tick_at(slug.clone(), 1, true).unwrap();
    let items = a.note(slug.clone()).unwrap().items;
    assert_eq!(items.len(), 3);
    assert_eq!(
        items.iter().map(|i| i.done).collect::<Vec<_>>(),
        vec![false, true, false],
        "the wrong one of two identical items was ticked"
    );
}

#[test]
fn a_reworded_item_keeps_its_tick_through_the_binding() {
    let root = scratch("reword-ffi");
    let a = Archive::open(root.clone()).unwrap();

    let Captured::Stored { slug } = a.capture("#shop: milk, eggs, bread".into()).unwrap() else {
        panic!("it stored");
    };
    a.tick_at(slug.clone(), 1, true).unwrap();
    a.edit(slug.clone(), "#shop: milk, free-range eggs, bread".into())
        .unwrap();

    let items = a.note(slug.clone()).unwrap().items;
    assert_eq!(items[1].text, "free-range eggs");
    assert!(items[1].done, "correcting a word discarded the tick");

    std::fs::remove_dir_all(&root).ok();
}

/// "gym every monday at 2" asks on the phone, and answering times it.
#[test]
fn a_bare_hour_is_asked_and_answered_from_the_phone() {
    let root = scratch("hour");
    let a = Archive::open(root.clone()).unwrap();
    let Captured::Stored { slug } = a.capture("go to the gym every monday at 2".into()).unwrap()
    else {
        panic!("spooled with a writable store")
    };

    assert_eq!(a.unclear_hour(slug.clone()).unwrap(), Some(2));
    let text = a.settle_hour(slug.clone(), true).unwrap();
    assert_eq!(text, "go to the gym every monday at 2pm");
    assert_eq!(
        a.unclear_hour(slug.clone()).unwrap(),
        None,
        "nothing left to ask"
    );
    assert!(
        a.settle_hour(slug.clone(), true).is_err(),
        "answering twice rewrote the text again"
    );

    std::fs::remove_dir_all(&root).ok();
}

/// Two weeks from the phone: a weekly series lands once a week, and only those
/// days are marked.
#[test]
fn a_fortnight_marks_the_days_something_happens() {
    let root = scratch("calendar");
    let a = Archive::open(root.clone()).unwrap();
    a.capture("gym every monday at 2pm".into()).unwrap();
    a.settle().unwrap();

    let today = a.today();
    let first = capture_time::FloatingDate::parse(&today).unwrap();
    let zone = capture_time::Zone::new("UTC").unwrap();
    let noon = capture_time::at_local(first, 12, 0, zone).unwrap();
    let last = capture_time::add_days(noon, zone, 13).unwrap().to_string();

    let days = a.calendar(today.clone(), last).unwrap();
    assert_eq!(days.len(), 14);
    let marked: Vec<_> = days.iter().filter(|d| d.busy).collect();
    assert_eq!(marked.len(), 2, "two Mondays in a fortnight: {days:?}");
    for d in marked {
        let gym = &d.items[0];
        assert!(gym.series && gym.occ.is_some() && !gym.skipped);
    }

    // The one-day agenda is the same read.
    let monday = &days.iter().find(|d| d.busy).unwrap().date;
    assert_eq!(a.agenda(monday.clone()).unwrap().len(), 1);

    std::fs::remove_dir_all(&root).ok();
}

/// Sick on Monday, from the phone: preview, move, skip, restore, done.
#[test]
fn one_occurrence_is_changed_from_the_phone() {
    let root = scratch("occurrences");
    let a = Archive::open(root.clone()).unwrap();
    let Captured::Stored { slug } = a.capture("gym every monday at 2pm".into()).unwrap() else {
        panic!("spooled with a writable store")
    };
    a.settle().unwrap();

    let today = a.today();
    let first = capture_time::FloatingDate::parse(&today).unwrap();
    let zone = capture_time::Zone::new("UTC").unwrap();
    let noon = capture_time::at_local(first, 12, 0, zone).unwrap();
    let last = capture_time::add_days(noon, zone, 13).unwrap().to_string();
    let days = a.calendar(today, last).unwrap();
    let monday = days.iter().find(|d| d.busy).expect("a Monday");
    let occ = monday.items[0].occ.expect("named");

    // Preview writes nothing.
    let seen = a
        .preview_move(slug.clone(), occ, "thursday 10am".into())
        .unwrap();
    assert!(seen.start_ms > occ, "Thursday after that Monday");
    assert_eq!(
        a.agenda(monday.date.clone()).unwrap().len(),
        1,
        "previewing moved it"
    );

    let landed = a
        .move_occurrence(slug.clone(), occ, "thursday 10am".into())
        .unwrap();
    assert_eq!(landed, seen, "the move went where the preview said");
    assert!(
        a.agenda(monday.date.clone()).unwrap().is_empty(),
        "still on Monday"
    );

    // A bare hour is asked about, not guessed.
    let err = a
        .preview_move(slug.clone(), occ, "friday at 2".into())
        .unwrap_err();
    assert!(format!("{err}").contains("2pm"), "{err}");

    // Skipping the moved one takes: it is shown as skipped wherever it was,
    // and on no day as happening. (It used to be a silent no-op, because a
    // move outranked any cancellation, however much later.)
    a.skip_occurrence(slug.clone(), occ).unwrap();
    let fortnight = a
        .calendar(days[0].date.clone(), days[13].date.clone())
        .unwrap();
    let all: Vec<_> = fortnight
        .iter()
        .flat_map(|d| &d.items)
        .filter(|e| e.occ == Some(occ))
        .collect();
    assert!(!all.is_empty() && all.iter().all(|e| e.skipped), "{all:?}");
    a.restore_occurrence(slug.clone(), occ).unwrap();
    let day = a.agenda(monday.date.clone()).unwrap();
    assert!(
        day.iter()
            .any(|e| e.occ == Some(occ) && !e.skipped && e.moved_from.is_none())
    );

    a.set_occurrence_done(slug.clone(), occ, true).unwrap();
    assert!(a.agenda(monday.date.clone()).unwrap()[0].done);

    std::fs::remove_dir_all(&root).ok();
}

/// The phone's grid arrives drawn: whole weeks, labels, and where the arrows go.
#[test]
fn a_month_view_is_ready_to_draw() {
    let root = scratch("view");
    let a = Archive::open(root.clone()).unwrap();
    a.capture("gym every monday at 2pm".into()).unwrap();
    a.settle().unwrap();

    let today = a.today();
    let v = a
        .calendar_view(today.clone(), capture_ffi::CalendarShape::Month)
        .unwrap();
    assert_eq!(v.days.len() % 7, 0, "whole weeks");
    assert!(v.days.iter().any(|d| d.today && d.date == today));
    assert!(
        v.days.iter().any(|d| !d.in_period),
        "a month rarely starts on a Monday"
    );
    let gym = v
        .days
        .iter()
        .filter(|d| d.in_period && d.busy)
        .flat_map(|d| &d.items)
        .next()
        .expect("a Monday this month with the gym");
    assert_eq!(gym.at, "14:00");

    let w = a
        .calendar_view(today.clone(), capture_ffi::CalendarShape::Week)
        .unwrap();
    assert_eq!(w.days.len(), 7);
    assert!(w.days[0].label.starts_with("Mon"));
    assert_ne!(w.previous, w.next);

    std::fs::remove_dir_all(&root).ok();
}
