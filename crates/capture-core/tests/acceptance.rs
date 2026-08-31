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

use capture_core::{
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
            wall: now + 10 * capture_core::hlc::LEAD_REPORT_MS,
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
    // makes `capture verify` able to notice a rewritten row.
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
    // would make `capture verify` useless to anyone who has destroyed anything.
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
fn a_device_paired_with_one_member_is_admitted_by_all() {
    // The O(n) property the roster-in-the-log exists for, and the thing that
    // makes pairing one physical gesture instead of one per pair of devices.
    // The desktop never met the phone; it learns of it by folding what the
    // laptop wrote.
    let mut laptop = testing::Origin::new(0);
    let desktop = testing::Origin::new(1);
    let phone = testing::Origin::new(2);

    let root = laptop.roster_root();
    let desktop_added = laptop.admits(&desktop);
    let phone_added = laptop.admits(&phone);

    let r = roster::build(&[root, desktop_added, phone_added]);

    assert_eq!(r.keys.len(), 3, "not every paired device was admitted");
    assert_eq!(r.key_for(&phone.dev()), Some(&phone.pubkey()));
    assert_eq!(r.key_for(&desktop.dev()), Some(&desktop.pubkey()));
    assert!(r.refused.is_empty(), "{:?}", r.refused);
}

#[test]
fn a_device_cannot_admit_itself_after_the_root() {
    // The hole this closes: if a non-root self-signed admission conferred
    // membership, anyone on the network could write one and join.
    let mut laptop = testing::Origin::new(0);
    let mut intruder = testing::Origin::new(1);

    let root = laptop.roster_root();
    // Later than the root, so which of the two is "first" is not a tiebreak.
    intruder.observe(&root, 1_700_000_001_000);
    let self_admission = intruder.roster_root();
    let id = self_admission.header.id();

    let r = roster::build(&[root, self_admission]);
    assert_eq!(r.keys.len(), 1);
    assert_eq!(r.refused.get(&id), Some(&roster::Refusal::NotTheFirstRoot));
}

#[test]
fn an_outsider_cannot_admit_anyone() {
    // Admission is what gates ingest, so a non-member able to write one would
    // be a non-member able to admit an attacker — and this is checkable
    // locally, without signatures, because the admitter's own origin must
    // already be in the roster.
    let mut laptop = testing::Origin::new(0);
    let mut outsider = testing::Origin::new(1);
    let friend = testing::Origin::new(2);

    let root = laptop.roster_root();
    let smuggled = outsider.admits(&friend);
    let id = smuggled.header.id();

    let r = roster::build(&[root, smuggled]);
    assert_eq!(r.keys.len(), 1, "an outsider admitted a device");
    assert_eq!(
        r.refused.get(&id),
        Some(&roster::Refusal::AdmitterIsNotAMember)
    );
}

#[test]
fn a_newly_admitted_device_may_admit_the_next_one() {
    // Any member may admit. There is no admin device, because a designated
    // admin is exactly the single point of failure serverless exists to avoid.
    let mut laptop = testing::Origin::new(0);
    let mut phone = testing::Origin::new(1);
    let tablet = testing::Origin::new(2);

    let root = laptop.roster_root();
    let phone_added = laptop.admits(&phone);
    // The phone can only admit the tablet after it has itself been admitted,
    // which means it has seen that event — so its own admission timestamps
    // after it. That is what makes one ordered pass enough.
    phone.observe(&phone_added, 1_700_000_001_000);
    let tablet_added = phone.admits(&tablet);

    let r = roster::build(&[root, phone_added, tablet_added]);
    assert_eq!(r.keys.len(), 3);
    assert_eq!(r.key_for(&tablet.dev()), Some(&tablet.pubkey()));
}

#[test]
fn admitting_the_same_device_twice_is_a_union() {
    // Two devices pair the same newcomer at once, or one admission arrives
    // twice by different routes. §5.3: concurrent adds for different devices
    // union, and this is the degenerate case of that.
    let mut laptop = testing::Origin::new(0);
    let mut desktop = testing::Origin::new(1);
    let phone = testing::Origin::new(2);

    let root = laptop.roster_root();
    let desktop_added = laptop.admits(&desktop);
    desktop.observe(&desktop_added, 1_700_000_001_000);

    let by_laptop = laptop.admits(&phone);
    let by_desktop = desktop.admits(&phone);

    let r = roster::build(&[root, desktop_added, by_laptop, by_desktop]);
    assert_eq!(r.keys.len(), 3);
    assert!(r.refused.is_empty(), "{:?}", r.refused);
}

#[test]
fn the_order_admissions_arrive_in_does_not_matter() {
    // They arrive over a wire, out of order and more than once. A roster that
    // depended on arrival order would have two devices disagree about who is
    // a member, which is a disagreement about which events are foldable at
    // all.
    let mut laptop = testing::Origin::new(0);
    let mut phone = testing::Origin::new(1);
    let tablet = testing::Origin::new(2);

    let root = laptop.roster_root();
    let phone_added = laptop.admits(&phone);
    phone.observe(&phone_added, 1_700_000_001_000);
    let tablet_added = phone.admits(&tablet);

    let forwards = roster::build(&[root.clone(), phone_added.clone(), tablet_added.clone()]);
    let backwards = roster::build(&[tablet_added, phone_added, root]);

    assert_eq!(forwards.keys, backwards.keys);
    assert_eq!(forwards.origins, backwards.origins);
    assert_eq!(backwards.keys.len(), 3);
}

// ------------------------------------------------------------ retirement ----

/// A stolen phone, and the two devices left behind.
fn mesh() -> (
    testing::Origin,
    testing::Origin,
    testing::Origin,
    Vec<Envelope>,
) {
    let mut laptop = testing::Origin::new(0);
    let mut desktop = testing::Origin::new(1);
    let phone = testing::Origin::new(2);

    let root = laptop.roster_root();
    let add_desktop = laptop.admits(&desktop);
    let add_phone = laptop.admits(&phone);
    desktop.observe(&add_desktop, 1_700_000_001_000);

    (laptop, desktop, phone, vec![root, add_desktop, add_phone])
}

#[test]
fn a_retired_key_is_no_longer_a_member() {
    let (mut laptop, _desktop, phone, mut events) = mesh();
    events.push(laptop.retires(&phone, &[(phone.dev(), 15677, EventId([1; 32]))]));

    let r = roster::build(&events);
    assert!(
        !r.keys.contains(&phone.pubkey()),
        "the retired key is still a member"
    );
    assert_eq!(
        r.head_of(&phone.dev()),
        Some((15677, EventId([1; 32]))),
        "the retirement did not record where the origin stops"
    );
}

#[test]
fn a_retired_devices_past_events_stay_valid() {
    // They were legitimately authored. Discarding them would mean losing real
    // notes, which is a worse outcome than the access the retirement closes.
    let (mut laptop, _d, phone, mut events) = mesh();
    events.push(laptop.retires(&phone, &[(phone.dev(), 15677, EventId([1; 32]))]));

    let r = roster::build(&events);
    assert_eq!(
        r.key_for(&phone.dev()),
        Some(&phone.pubkey()),
        "the origin lost the key its past events must still be checked against"
    );
}

#[test]
fn retirement_beats_a_concurrent_admission() {
    // Deliberately the opposite of the add-wins rule for tags. For content,
    // losing data is the worse outcome, so adds win; for access, granting
    // unintended access is worse, so revocation wins. Without this a stolen
    // device need only be re-added by any member — or by itself, once it had
    // compromised one.
    let (mut laptop, mut desktop, phone, mut events) = mesh();
    let retire = laptop.retires(&phone, &[(phone.dev(), 15677, EventId([1; 32]))]);
    desktop.observe(&retire, 1_700_000_002_000);
    let re_add = desktop.admits(&phone);
    let re_add_id = re_add.header.id();
    events.push(retire);
    events.push(re_add);

    let r = roster::build(&events);
    assert!(
        !r.keys.contains(&phone.pubkey()),
        "an ordinary admission undid a retirement"
    );
    assert_eq!(
        r.refused.get(&re_add_id),
        Some(&roster::Refusal::RetiredKey)
    );
}

#[test]
fn the_lowest_head_wins_between_two_retirements() {
    // Two devices retiring the same stolen phone name whatever seq each had
    // seen. Lowest wins: it is the safe direction, excluding the events whose
    // provenance is in doubt. Without a rule the replicas permanently disagree
    // about which events exist, and nothing flags it — divergences tracks note
    // bodies only.
    let (mut laptop, mut desktop, phone, mut events) = mesh();
    events.push(laptop.retires(&phone, &[(phone.dev(), 15677, EventId([1; 32]))]));
    events.push(desktop.retires(&phone, &[(phone.dev(), 15000, EventId([2; 32]))]));

    let r = roster::build(&events);
    assert_eq!(r.head_of(&phone.dev()), Some((15000, EventId([2; 32]))));
}

#[test]
fn the_lowest_head_wins_whichever_order_they_arrive() {
    let (mut laptop, mut desktop, phone, base) = mesh();
    let high = laptop.retires(&phone, &[(phone.dev(), 15677, EventId([1; 32]))]);
    let low = desktop.retires(&phone, &[(phone.dev(), 15000, EventId([2; 32]))]);

    let mut forwards = base.clone();
    forwards.extend([high.clone(), low.clone()]);
    let mut backwards = base;
    backwards.extend([low, high]);

    assert_eq!(
        roster::build(&forwards).head_of(&phone.dev()),
        roster::build(&backwards).head_of(&phone.dev()),
        "two replicas disagree about where a retired origin stops"
    );
}

#[test]
fn a_reissue_widens_where_a_concurrent_retirement_would_narrow() {
    // The two rules in §5.3 are in tension: lowest-head-wins is right between
    // concurrent retirements, and wrong for a reissue, which exists precisely
    // to name a *later* head once the retired device turns out to have
    // propagated further. Naming the retirement being replaced separates them.
    let (mut laptop, _d, phone, base) = mesh();
    let narrow = laptop.retires(&phone, &[(phone.dev(), 15000, EventId([2; 32]))]);
    let wider = laptop.retires_superseding(
        &phone,
        &[(phone.dev(), 15677, EventId([1; 32]))],
        Some(narrow.header.id()),
    );

    let mut events = base;
    events.extend([narrow, wider]);
    assert_eq!(
        roster::build(&events).head_of(&phone.dev()),
        Some((15677, EventId([1; 32])))
    );
}

#[test]
fn a_reissue_wins_whichever_order_it_arrives_in() {
    // Superseded retirements are collected before any is applied. Otherwise a
    // replica that saw the reissue first would then apply the older, narrower
    // claim on top of it and disagree with one that saw them the other way
    // round — a permanent disagreement about which events exist.
    let (mut laptop, _d, phone, base) = mesh();
    let narrow = laptop.retires(&phone, &[(phone.dev(), 15000, EventId([2; 32]))]);
    let wider = laptop.retires_superseding(
        &phone,
        &[(phone.dev(), 15677, EventId([1; 32]))],
        Some(narrow.header.id()),
    );

    let mut forwards = base.clone();
    forwards.extend([narrow.clone(), wider.clone()]);
    let mut backwards = base;
    backwards.extend([wider, narrow]);

    assert_eq!(
        roster::build(&forwards).head_of(&phone.dev()),
        roster::build(&backwards).head_of(&phone.dev())
    );
}

#[test]
fn the_last_member_cannot_retire_itself() {
    // A roster with nobody in it can never admit anyone again: device_add
    // needs a signature from a current member.
    let mut laptop = testing::Origin::new(0);
    let root = laptop.roster_root();
    let suicide = laptop.retires_self();
    let id = suicide.header.id();

    let r = roster::build(&[root, suicide]);
    assert!(r.is_bootstrapped(), "the roster was emptied");
    assert_eq!(
        r.refused.get(&id),
        Some(&roster::Refusal::WouldEmptyTheRoster)
    );
}

#[test]
fn a_device_does_not_accept_being_retired_by_someone_else() {
    // The first lockout guard. Any member may retire any member, so one
    // malfunctioning device could retire every other; a device therefore does
    // not lock itself out on another's say-so alone. Replicas disagree
    // afterwards, and that is the intent.
    let (mut laptop, desktop, _p, mut events) = mesh();
    events.push(laptop.retires(&desktop, &[(desktop.dev(), 40, EventId([3; 32]))]));

    let mut r = roster::build(&events);
    assert!(
        !r.keys.contains(&desktop.pubkey()),
        "as every other device sees it"
    );

    r.honour_self(&desktop.pubkey());
    assert!(
        r.keys.contains(&desktop.pubkey()),
        "the device locked itself out on someone else's say-so"
    );
}

#[test]
fn a_device_does_accept_a_retirement_it_issued_itself() {
    let (mut laptop, desktop, _p, mut events) = mesh();
    let d_key = desktop.pubkey();
    let mut desktop = desktop;
    events.push(laptop.admits(&desktop));
    events.push(desktop.retires_self());

    let mut r = roster::build(&events);
    r.honour_self(&d_key);
    assert!(
        !r.keys.contains(&d_key),
        "a device ignored its own retirement"
    );
}

#[test]
fn a_stolen_device_is_unaffected_by_the_self_guard() {
    // The guard protects the device from locking *itself* out. What retiring a
    // stolen phone achieves is that its peers stop accepting its events, and
    // they still do — the phone believing otherwise changes nothing.
    let (mut laptop, desktop, phone, mut events) = mesh();
    events.push(laptop.retires(&phone, &[(phone.dev(), 15677, EventId([1; 32]))]));

    let mut as_the_desktop_sees_it = roster::build(&events);
    as_the_desktop_sees_it.honour_self(&desktop.pubkey());
    assert!(!as_the_desktop_sees_it.keys.contains(&phone.pubkey()));
}

// ------------------------------------------------------------ readmission ----

#[test]
fn two_signers_can_readmit_a_device_after_a_lockout() {
    // Every honest device retired by one compromised member. device_add cannot
    // help: it needs a current member's signature and there are none. Pairing
    // cannot either — it bootstraps transport keys, while the roster entry
    // that gates ingest still has to come from a member.
    let (mut laptop, mut desktop, _p, mut events) = mesh();
    events.push(laptop.retires(&desktop, &[(desktop.dev(), 40, EventId([3; 32]))]));
    events.push(desktop.readmits_self(Some(&laptop)));

    let r = roster::build(&events);
    assert!(
        r.keys.contains(&desktop.pubkey()),
        "a co-signed re-admission did not restore membership"
    );
}

#[test]
fn a_readmission_signed_twice_by_one_device_is_refused() {
    // Two signatures from one device are one device's say-so written twice,
    // which is exactly the bar the second signer exists to raise.
    let (mut laptop, mut desktop, _p, mut events) = mesh();
    events.push(laptop.retires(&desktop, &[(desktop.dev(), 40, EventId([3; 32]))]));
    let forged = desktop.readmits_self(None);
    let id = forged.header.id();
    events.push(forged);

    let r = roster::build(&events);
    assert!(!r.keys.contains(&desktop.pubkey()));
    assert_eq!(r.refused.get(&id), Some(&roster::Refusal::NotTwoSigners));
}

#[test]
fn a_readmission_with_a_bad_cosignature_is_refused() {
    let (mut laptop, mut desktop, phone, mut events) = mesh();
    events.push(laptop.retires(&desktop, &[(desktop.dev(), 40, EventId([3; 32]))]));

    let mut forged = desktop.readmits_self(Some(&phone));
    // Corrupt the co-signature, leaving everything else intact.
    let mut body: capture_core::body::ReadmitBody =
        minicbor::decode(forged.body.as_ref().unwrap()).unwrap();
    body.cosign[0] ^= 0xFF;
    let mut buf = Vec::new();
    minicbor::encode(&body, &mut buf).unwrap();
    forged.header.bhash = capture_core::id::BodyHash::of(&buf);
    forged.body = Some(buf);
    let id = forged.header.id();
    events.push(forged);

    let r = roster::build(&events);
    assert!(!r.keys.contains(&desktop.pubkey()));
    assert_eq!(r.refused.get(&id), Some(&roster::Refusal::BadCosignature));
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
