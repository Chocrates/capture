//! The properties M1 is not finished without.
//!
//! # Why these are gathered here rather than left where they were written
//!
//! Every one of these is also checked somewhere in the unit tests, next to the
//! code it constrains, which is where it belongs while that code is being
//! written. What that arrangement cannot do is answer "is the thing done" —
//! the exit criteria end up spread across a dozen files, stated in the
//! vocabulary of whatever module happened to need them, and nobody can read
//! the list.
//!
//! So this file states them once, in the language of the design rather than the
//! implementation, and each one names what breaks if it does not hold. It is
//! deliberately redundant. A duplicated assertion costs milliseconds; an exit
//! criterion nobody can find costs an argument about whether the milestone is
//! finished.
//!
//! # What is not here
//!
//! Anything needing a store, a process or a terminal. Those are integration
//! suites of their own — `tests/redact.rs`, `tests/crash.rs`, `tests/pty.rs`,
//! `tests/roundtrip.rs`, `tests/spool.rs`, `tests/tamper.rs`, and the CLI's
//! `tests/scenarios.rs`. This file is the pure half: things that are true of
//! the event model regardless of where it is stored.

use brain_core::{
    Dev, EventId, Hlc, HlcState, Kind,
    body::{MuteBody, MuteScope, RedactBody, RedactScope},
    envelope::{Envelope, Verdict, verify_chain},
    fold, roster, slug, testing,
};

// ---------------------------------------------------------------- ordering ----

#[test]
fn the_fold_is_a_function_of_the_event_set_alone() {
    // The flagship property. If this fails, two devices holding identical
    // events show different things, and every other guarantee here is
    // decoration.
    for seed in 1..12u64 {
        let s = testing::mixed(seed, 3, 5, 40);
        let base = fold::fold(&s.events).canonical();
        for shuffle in [7u64, 99, 12_345, 2, 808] {
            assert_eq!(
                fold::fold(&s.shuffled(shuffle)).canonical(),
                base,
                "seed {seed}: arrival order changed the result"
            );
        }
    }
}

#[test]
fn the_clock_orders_events_totally() {
    // A partial order leaves last-writer-wins with no winner, and every
    // conflict rule in the design resolves against this comparison.
    let a = Hlc {
        wall: 10,
        counter: 0,
        origin: Dev([1; 32]),
    };
    let b = Hlc {
        wall: 10,
        counter: 0,
        origin: Dev([2; 32]),
    };
    assert_ne!(
        a.cmp(&b),
        core::cmp::Ordering::Equal,
        "origin breaks the tie"
    );

    let later = Hlc {
        wall: 11,
        counter: 0,
        origin: Dev([1; 32]),
    };
    assert!(later > a);
}

#[test]
fn a_stamp_never_goes_backwards() {
    // Including across a restart and a clock correction, which is where it
    // would otherwise happen in the first week on a single device.
    let mut s = HlcState::new();
    let d = Dev([1; 32]);
    let first = s.issue(1_000_000, d);
    let (wall, counter) = s.persist();

    let mut resumed = HlcState::restore(wall, counter);
    let after_restart = resumed.issue(999_000, d); // the clock went backwards
    assert!(after_restart > first);
}

#[test]
fn a_wrong_clock_never_refuses_a_capture() {
    // Adoption is mandatory, so a peer with a bad clock really does drag this
    // one forward. Refusing to author in that state would close the capture
    // path on a machine that is working perfectly, because of a clock
    // elsewhere.
    let mut s = HlcState::new();
    let now = 1_000_000;
    s.observe(
        &Hlc {
            wall: now + 10 * brain_core::hlc::LEAD_REPORT_MS,
            counter: 0,
            origin: Dev([2; 32]),
        },
        now,
    );
    let stamp = s.issue(now, Dev([1; 32]));
    assert!(stamp.wall > now, "the adopted time is carried");
    assert!(s.lead_ms(now) > 0, "and the condition is measurable");
}

// ---------------------------------------------------------------- identity ----

#[test]
fn an_event_is_named_by_what_it_says() {
    // Change any part of the header and the name changes with it, which is what
    // makes `brain verify` able to notice a rewritten row.
    let mut d = testing::Origin::new(0);
    let e = d.author(
        Kind::CAPTURE,
        None,
        None,
        testing::capture_body(1, "something worth keeping"),
        1_700_000_000_000,
    );
    let original = e.header.id();

    let mut altered = e.clone();
    altered.header.kind = Kind::EDIT;
    assert_ne!(altered.header.id(), original);
}

#[test]
fn a_redacted_event_still_verifies() {
    // The body appears in the header only as a hash, so an event with no body
    // recomputes its id and checks out. A redacted archive that read as damaged
    // would make `brain verify` useless to anyone who has destroyed anything.
    let mut d = testing::Origin::new(0);
    let mut e = d.author(
        Kind::CAPTURE,
        None,
        None,
        testing::capture_body(1, "about to be destroyed"),
        1_700_000_000_000,
    );
    let key = d.pubkey();
    assert_eq!(e.verify(&key), Verdict::Authentic);

    e.body = None;
    assert_eq!(
        e.verify(&key),
        Verdict::Authentic,
        "destroying the payload must not make the event unverifiable"
    );
}

#[test]
fn a_tampered_body_fails_its_commitment() {
    let mut d = testing::Origin::new(0);
    let mut e = d.author(
        Kind::CAPTURE,
        None,
        None,
        testing::capture_body(1, "the original text"),
        1_700_000_000_000,
    );
    e.body = Some(testing::capture_body(1, "something else entirely"));
    assert!(matches!(e.verify(&d.pubkey()), Verdict::Rejected(_)));
}

#[test]
fn an_origin_chain_is_walkable_and_gaps_are_visible() {
    // A gap in an origin's numbering is unrecoverable — a device publishes one
    // number per origin and cannot describe a hole — so it has to be
    // detectable.
    let mut d = testing::Origin::new(0);
    let run: Vec<Envelope> = (1..=4)
        .map(|i| {
            d.author(
                Kind::CAPTURE,
                None,
                None,
                testing::capture_body(i, &format!("event {i}")),
                1_700_000_000_000 + i as u64 * 1000,
            )
        })
        .collect();
    assert!(verify_chain(&run).is_ok());

    let with_hole = [run[0].clone(), run[2].clone()];
    assert!(verify_chain(&with_hole).is_err());
}

// ------------------------------------------------------------- convergence ----

#[test]
fn a_concurrent_edit_is_kept_rather_than_dropped() {
    // Data loss is the cardinal sin. Two devices editing without seeing each
    // other produce two versions, both retained, and the note says so.
    let s = testing::branch_vs_single(1);
    let p = fold::fold(&s.events);
    let n = &p.notes[&s.notes[0]];
    assert!(
        !n.divergences.is_empty(),
        "an unreconciled edit must be reported, not silently discarded"
    );
    assert!(n.versions.len() >= 3, "and every version is still there");
}

#[test]
fn tags_converge_add_wins() {
    // A tag added on a device that never saw the removal must survive it,
    // whichever order the two arrive in.
    let s = testing::concurrent_tag(3);
    let base = fold::fold(&s.events).canonical();
    for shuffle in [1u64, 42, 9_999] {
        assert_eq!(fold::fold(&s.shuffled(shuffle)).canonical(), base);
    }
}

#[test]
fn a_name_denotes_exactly_one_note() {
    // Slugs and aliases share one namespace. Two notes answering to one name
    // would make a written link mean different things depending on which
    // namespace a reader consulted first.
    let mut d = testing::Origin::new(0);
    let a = d.author(
        Kind::CAPTURE,
        None,
        None,
        testing::capture_body(1, "the first note here"),
        1_700_000_000_000,
    );
    let b = d.author(
        Kind::CAPTURE,
        None,
        None,
        testing::capture_body(2, "the second note here"),
        1_700_000_001_000,
    );
    let (a_id, b_id) = (a.header.id(), b.header.id());
    let first = d.author(
        Kind::ALIAS,
        Some(a_id),
        None,
        testing::alias_body(3, Some("shared")),
        1_700_000_002_000,
    );
    let second = d.author(
        Kind::ALIAS,
        Some(b_id),
        None,
        testing::alias_body(4, Some("shared")),
        1_700_000_003_000,
    );

    let p = fold::fold(&[a, b, first, second]);
    let holders = p
        .notes
        .values()
        .filter(|n| n.alias.as_deref() == Some("shared"))
        .count();
    assert_eq!(holders, 1);
}

#[test]
fn a_link_records_what_it_meant_when_it_was_written() {
    // Storing only the name makes every link a lookup against the present, so a
    // name that later moves silently repoints old references at content their
    // author never saw.
    let mut d = testing::Origin::new(0);
    let target = d.author(
        Kind::CAPTURE,
        None,
        None,
        testing::capture_body(1, "budget planning for next quarter"),
        1_700_000_000_000,
    );
    let target_id = target.header.id();
    let name = fold::fold(core::slice::from_ref(&target)).notes[&target_id]
        .slug
        .clone();

    let linker = d.author(
        Kind::CAPTURE,
        None,
        None,
        testing::capture_body(2, &format!("see [[{name}]] for figures")),
        1_700_000_001_000,
    );
    let linker_id = linker.header.id();

    let p = fold::fold(&[target, linker]);
    let link = &p.notes[&linker_id].links[0];
    assert_eq!(link.name, name, "the name as written survives");
    assert_eq!(link.resolved, Some(target_id), "and what it meant");
}

// ----------------------------------------------------------------- naming ----

#[test]
fn a_name_is_stable_and_unique() {
    // Names are written into other notes' text, where nothing can rewrite them.
    // A name that changed, or that two notes shared, would break links with no
    // cleanup path.
    let s = testing::mixed(5, 3, 4, 60);
    let p = fold::fold(&s.events);

    let mut seen = std::collections::BTreeSet::new();
    for n in p.notes.values() {
        assert!(seen.insert(n.slug.clone()), "duplicate name {}", n.slug);
    }

    // And folding again gives the same names.
    let again = fold::fold(&s.shuffled(31));
    for (tgt, n) in &p.notes {
        assert_eq!(&again.notes[tgt].slug, &n.slug);
    }
}

#[test]
fn a_credential_shaped_capture_is_not_named_after_its_contents() {
    // A name propagates into every note that links here, where no redaction can
    // reach it. The predicate over-flags on purpose.
    let s = slug::derive(
        "my aws secret key is AKIAIOSFODNN7EXAMPLE",
        "2026-08-21",
        &EventId([7; 32]),
    );
    assert!(!s.text.contains("aws"), "got {}", s.text);
    assert!(!s.text.contains("AKIA"), "got {}", s.text);
}

// ------------------------------------------------------------- destruction ----

#[test]
fn a_redaction_alone_destroys_nothing() {
    // The cancellation window, and the reason it can exist: if ordering
    // destruction destroyed anything, cancelling would have nothing to save.
    let s = testing::mixed(11, 1, 1, 1);
    let tgt = s.notes[0];
    let mut d = testing::Origin::new(9);
    let mut events = s.events.clone();
    events.push(d.author(
        Kind::REDACT,
        Some(tgt),
        None,
        testing::encode(&RedactBody {
            nonce: [0; 16],
            scope: RedactScope::WholeNote,
            targets: vec![tgt],
        }),
        1_700_000_500_000,
    ));

    let n = &fold::fold(&events).notes[&tgt];
    assert!(n.text.is_some(), "still readable during the window");
    assert!(n.redaction_pending.is_some(), "and queued");
}

#[test]
fn destroyed_content_stays_destroyed_whatever_arrives_later() {
    // A redaction is retained so that matching events turning up afterwards are
    // destroyed too. Otherwise "destroyed" means "destroyed unless something
    // was slow".
    let s = testing::mixed(12, 1, 1, 2);
    let tgt = s.notes[0];
    let mut d = testing::Origin::new(9);

    let purged: Vec<Envelope> = s
        .events
        .iter()
        .cloned()
        .map(|mut e| {
            if e.header.tgt == Some(tgt) || e.header.id() == tgt {
                e.body = None;
            }
            e
        })
        .collect();

    let mut events = purged;
    events.push(d.author(
        Kind::REDACT,
        Some(tgt),
        None,
        testing::encode(&RedactBody {
            nonce: [0; 16],
            scope: RedactScope::WholeNote,
            targets: vec![tgt],
        }),
        1_700_000_500_000,
    ));

    let n = &fold::fold(&events).notes[&tgt];
    assert!(n.is_redacted());
    assert_eq!(n.text, None);
    assert!(
        n.slug.starts_with("redacted-"),
        "and the name carries nothing"
    );
}

#[test]
fn a_destroyed_note_still_exists() {
    // Losing the text is the point. Losing the note would take its place in the
    // timeline and every reference to it, and the archive would quietly be
    // missing an entry rather than a body.
    let mut d = testing::Origin::new(0);
    let mut cap = d.author(
        Kind::CAPTURE,
        None,
        None,
        testing::capture_body(1, "about to be destroyed"),
        1_700_000_000_000,
    );
    let tgt = cap.header.id();
    cap.body = None;

    let p = fold::fold(&[cap]);
    assert!(p.notes.contains_key(&tgt));
    assert!(
        p.held.is_empty(),
        "an absent body is a known state, not a fault"
    );
}

// ------------------------------------------------------------ suppression ----

#[test]
fn a_mute_is_a_rule_rather_than_a_property() {
    // A tag mute covers whatever carries the tag when the question is asked, so
    // a note tagged afterwards is covered.
    let mut d = testing::Origin::new(0);
    let cap = d.author(
        Kind::CAPTURE,
        None,
        None,
        testing::capture_body(1, "a note tagged after the rule"),
        1_700_000_000_000,
    );
    let tgt = cap.header.id();
    let mute = d.author(
        Kind::MUTE,
        None,
        None,
        testing::encode(&MuteBody {
            nonce: [5; 16],
            scope: MuteScope::Tag("noise".into()),
            until: None,
        }),
        1_700_000_001_000,
    );
    let tag = d.author(
        Kind::TAG,
        Some(tgt),
        None,
        testing::tag_body(6, "noise"),
        1_700_000_002_000,
    );

    assert_eq!(
        fold::fold(&[cap, mute, tag]).notes[&tgt].muted_until,
        Some(0)
    );
}

// ------------------------------------------------------------- membership ----

#[test]
fn only_one_root_confers_membership() {
    // A second self-signed admission is authentic and keeps its position — a
    // refusal would leave a hole in its origin's numbering — and confers
    // nothing.
    let mut a = testing::Origin::new(0);
    let mut b = testing::Origin::new(1);
    let r = roster::build(&[a.roster_root(), b.roster_root()]);
    assert_eq!(r.keys.len(), 1);
    assert_eq!(r.refused.len(), 1);
}

#[test]
fn a_restored_device_stays_a_member() {
    // Membership is keyed by public key, not by origin, precisely so that a
    // restore does not evict the device. The claim is what binds the new origin
    // to the existing key.
    let mut first = testing::Origin::new(0);
    let root = first.roster_root();
    let mut restored = first.restored([9; 16]);
    let claim = restored.origin_claim();

    let r = roster::build(&[root, claim]);
    assert_eq!(r.keys.len(), 1, "a restore does not add a member");
    assert_eq!(r.key_for(&restored.dev()), Some(&first.pubkey()));
}

#[test]
fn the_roster_secret_is_the_latest_rotation() {
    let mut a = testing::Origin::new(0);
    let root = a.roster_root();
    let first = a.roster_key([1; 32]);
    let second = a.roster_key([2; 32]);

    let r = roster::build(&[root, first, second]);
    assert_eq!(
        r.secret,
        Some([2; 32]),
        "a device advertising under a superseded secret is a device its peers \
         cannot recognise"
    );
}

#[test]
fn an_archive_with_no_rotation_has_no_secret() {
    // Every archive written before sync existed. `None` is the true statement
    // about it — it has never advertised — and inventing one on read would
    // have each device invent a different one.
    let mut a = testing::Origin::new(0);
    let root = a.roster_root();
    assert_eq!(roster::build(&[root]).secret, None);
}

#[test]
fn an_outsider_cannot_choose_what_the_mesh_advertises_under() {
    // The secret is what makes the broadcast recognisable only to members. A
    // non-member able to set it could make every device advertise under a
    // value it knows, which is the correlation leak with extra steps.
    let mut a = testing::Origin::new(0);
    let mut stranger = testing::Origin::new(1);
    let root = a.roster_root();
    let mine = a.roster_key([1; 32]);
    let theirs = stranger.roster_key([9; 32]);
    let theirs_id = theirs.header.id();

    let r = roster::build(&[root, mine, theirs]);
    assert_eq!(r.secret, Some([1; 32]));
    assert_eq!(
        r.refused.get(&theirs_id),
        Some(&roster::Refusal::KeyIsNotAMember),
        "a non-member's rotation was ignored silently rather than reported"
    );
}

#[test]
fn an_unknown_kind_is_kept_rather_than_discarded() {
    // From a newer build. Storing it is what makes an upgrade able to fold it
    // later; discarding it would lose data on the older device permanently.
    let s = testing::unknown_kind(1);
    let p = fold::fold(&s.events);
    assert!(!p.held.is_empty(), "held, not dropped");
}
