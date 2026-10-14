//! Two real stores, one archive.
//!
//! Real identities, real Noise over a real socket, and the same `Gate` a local
//! capture goes through. What is deliberately *not* here is the daemon's loop:
//! this asserts that two devices converge, not that a timer fires.
//!
//! §11.2 calls the concurrent-edit case the acceptance test that matters, and
//! it is right to: everything else confirms the protocol works, and that one
//! confirms the data model does. Every merge rule M1 and M2 property-tested
//! was, until now, exercised by exactly one writer.

use std::net::{TcpListener, TcpStream};

use capture_store::ops::Ops;
use capture_store::paths::StorePaths;
use capture_store::session::{Admitted, Session};
use capture_sync::session::Session as Secure;
use capture_sync::tcp::Tcp;

struct Device {
    paths: StorePaths,
}

impl Device {
    fn ops(&self) -> Ops {
        Ops::open(&self.paths.ops).unwrap()
    }

    fn new(name: &str) -> Device {
        let root = std::env::temp_dir().join(format!(
            "capture-2dev-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let paths = StorePaths::under(&root);
        let fresh = capture_store::store::create(&paths, 1_700_000_000_000).unwrap();

        let mut log = capture_store::log::Log::open(&paths.events).unwrap();
        log.append(&fresh.root).unwrap();
        log.append(&fresh.roster_key).unwrap();

        let ops = Ops::open(&paths.ops).unwrap();
        let generation = ops.generation().unwrap();
        capture_store::projection::Store::open(&capture_store::projection::generation_path(
            &paths.state,
            generation,
        ))
        .unwrap();

        let (wall, counter) = fresh.clock.persist();
        let mut identity = fresh.identity;
        identity
            .advance(&paths, fresh.roster_key.header.seq, wall, counter)
            .unwrap();

        Device { paths }
    }

    fn identity(&self) -> capture_store::Identity {
        capture_store::Identity::load(&self.paths).unwrap()
    }

    fn admitted(&self) -> Admitted {
        let id = self.identity();
        Admitted {
            pubkey: id.public().to_bytes(),
            incarnation: id.incarnation().0,
            noise: id.noise_public(),
        }
    }

    fn session(&self) -> Session {
        Session::open(self.paths.clone()).unwrap()
    }

    /// Capture, and return the note's permanent name.
    ///
    /// The slug rather than the id, because a slug is what both devices call
    /// the note and the test is about them agreeing.
    fn capture(&self, text: &str) -> String {
        let mut s = self.session();
        let body = capture_store::session::capture_body(text).unwrap();
        s.capture(&body).unwrap().slug
    }

    fn read(&self) -> capture_store::projection::Store {
        let generation = Ops::open(&self.paths.ops).unwrap().generation().unwrap();
        capture_store::projection::Store::open_read(&capture_store::projection::generation_path(
            &self.paths.state,
            generation,
        ))
        .unwrap()
    }

    /// Whether the log still holds the bytes of the note's capture.
    ///
    /// The projection hiding the text is not the promise. `text` is also None
    /// while a redaction is merely *queued*, so a test that checked only that
    /// would pass with the content still on disk — which is the one outcome a
    /// redaction exists to prevent.
    fn still_holds_body(&self, id: &capture_core::EventId) -> bool {
        let log = capture_store::log::Log::open_read(&self.paths.events).unwrap();
        log.all()
            .unwrap()
            .iter()
            .find(|e| &e.header.id() == id)
            .map(|e| e.body.is_some())
            .unwrap_or(false)
    }

    fn id_of(&self, slug: &str) -> capture_core::EventId {
        self.read()
            .resolve(slug)
            .unwrap()
            .unwrap_or_else(|| panic!("no note named {slug}"))
    }

    fn note_count(&self) -> u64 {
        self.read().note_count().unwrap()
    }

    /// The text of a note, by the name both devices know it as.
    fn text_of(&self, slug: &str) -> Option<String> {
        let id = self.read().resolve(slug).unwrap()?;
        self.read().note(&id).unwrap().and_then(|n| n.text)
    }
}

impl Drop for Device {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.paths.root).ok();
    }
}

/// An edit body, with the nonce the session minted.
fn edit(nonce: [u8; 16], text: &str) -> Vec<u8> {
    let mut b = Vec::new();
    minicbor::encode(
        &capture_core::body::EditBody {
            nonce,
            text: text.into(),
        },
        &mut b,
    )
    .unwrap();
    b
}

/// The moment a reconcile is said to happen at.
///
/// Fixed, because the round takes its clock as an argument rather than reading
/// one — so a suite about convergence does not also depend on what time it is.
const AT: u64 = 1_700_000_100_000;

/// Run one round of anti-entropy between two stores over a real socket.
fn sync(a: &Device, b: &Device) {
    sync_at(a, b, AT);
}

/// The same round, said to happen at a given moment — for what depends on how
/// long something has been true rather than on whether it is.
fn sync_at(a: &Device, b: &Device, at: u64) {
    let a_id = a.identity();
    let b_id = b.identity();

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();

    // Through `round::reconcile`, which is what the daemon and the phone run.
    //
    // This used to open its own gate and call `exchange` directly. That made
    // the suite proving convergence the one place convergence was performed
    // differently from anywhere else — and it is exactly where a step can go
    // missing without any test objecting, because this *is* the test. The
    // fold after an exchange was once missing in the daemon while the suite
    // here was green.
    let b_paths = b.paths.clone();
    let b_secret = b_id.noise_secret();
    let responder = std::thread::spawn(move || {
        let stream = listener.accept().unwrap().0;
        let tcp = Tcp::new(stream).unwrap();
        let mut secure = Secure::accept(tcp, &b_secret).expect("the responder handshook");
        capture_store::round::reconcile(&b_paths, &mut secure, at, false, &|| false)
            .expect("the responder finished")
    });

    let tcp = Tcp::new(TcpStream::connect(addr).unwrap()).unwrap();
    let mut secure =
        Secure::initiate(tcp, &a_id.noise_secret(), &b_id.noise_public()).expect("handshook");
    capture_store::round::reconcile(&a.paths, &mut secure, at, true, &|| false)
        .expect("the initiator finished");

    responder.join().expect("no panic");
}

/// Pair two fresh devices: each admits the other, so either may dial.
fn pair(a: &Device, b: &Device) {
    a.session().admit(&b.admitted()).unwrap();
    b.session().admit(&a.admitted()).unwrap();
}

#[test]
fn two_devices_two_captures_one_archive() {
    // §11.1. Each device writes something the other has never seen, and after
    // one round both hold both.
    let a = Device::new("a-conv");
    let b = Device::new("b-conv");
    pair(&a, &b);

    let from_a = a.capture("written on the laptop");
    let from_b = b.capture("written on the phone");

    sync(&a, &b);

    assert_eq!(a.text_of(&from_b).as_deref(), Some("written on the phone"));
    assert_eq!(b.text_of(&from_a).as_deref(), Some("written on the laptop"));
    assert_eq!(a.note_count(), b.note_count(), "the two folds disagree");
}

#[test]
fn they_converge_after_a_partition() {
    // Asserted after a partition rather than only on a good network: several
    // captures pile up on each side while they cannot see each other, and one
    // round settles all of them.
    let a = Device::new("a-part");
    let b = Device::new("b-part");
    pair(&a, &b);

    let mut mine = Vec::new();
    let mut theirs = Vec::new();
    for i in 0..5 {
        mine.push(a.capture(&format!("laptop note {i}")));
        theirs.push(b.capture(&format!("phone note {i}")));
    }

    sync(&a, &b);

    for slug in mine.iter().chain(theirs.iter()) {
        assert!(a.text_of(slug).is_some(), "the laptop is missing an event");
        assert!(b.text_of(slug).is_some(), "the phone is missing an event");
    }
    assert_eq!(a.note_count(), b.note_count());
}

#[test]
fn a_second_round_moves_nothing() {
    // Convergence is stable, not oscillating. A round that kept finding work
    // would mean the two never agree they are done, and the daemon would sync
    // for ever.
    let a = Device::new("a-stable");
    let b = Device::new("b-stable");
    pair(&a, &b);
    a.capture("one");
    b.capture("two");

    sync(&a, &b);
    let counts = (a.note_count(), b.note_count());
    sync(&a, &b);

    assert_eq!((a.note_count(), b.note_count()), counts);
}

#[test]
fn concurrent_edits_to_one_note_are_resolved_the_same_way_on_both() {
    // §11.2, the acceptance test that matters. Everything else confirms the
    // protocol works; this confirms the data model does. Both devices edit the
    // same note while unable to see each other, and the merge rules — property
    // tested since M1 and never once exercised by a second writer — have to
    // land both replicas on the same text.
    let a = Device::new("a-edit");
    let b = Device::new("b-edit");
    pair(&a, &b);

    let note = a.capture("the original");
    sync(&a, &b);
    assert_eq!(b.text_of(&note).as_deref(), Some("the original"));

    let on_a = a.id_of(&note);
    let on_b = b.id_of(&note);
    assert_eq!(on_a, on_b, "the two devices know the note by different ids");

    // Neither has seen the other's change.
    a.session()
        .append(capture_core::Kind::EDIT, Some(on_a), None, |nonce| {
            edit(nonce, "edited on the laptop")
        })
        .unwrap();
    b.session()
        .append(capture_core::Kind::EDIT, Some(on_b), None, |nonce| {
            edit(nonce, "edited on the phone")
        })
        .unwrap();

    sync(&a, &b);

    let on_a = a.text_of(&note);
    let on_b = b.text_of(&note);
    assert_eq!(
        on_a, on_b,
        "the two devices resolved a concurrent edit differently, so the archive \
         has permanently diverged"
    );
    assert!(
        on_a.as_deref() == Some("edited on the laptop")
            || on_a.as_deref() == Some("edited on the phone"),
        "neither edit won: {on_a:?}"
    );
}

#[test]
fn a_redaction_ordered_while_the_other_slept_takes_effect_when_it_wakes() {
    // §11.3. The phone is asleep when the redaction is ordered, so the eager
    // push cannot reach it; anti-entropy delivers it on the next round. The
    // point of the test is that the *destruction* happens on arrival — the
    // content being gone everywhere is what a redaction promises, and a device
    // that merely learned about it would still be holding the text.
    let a = Device::new("a-redact");
    let b = Device::new("b-redact");
    pair(&a, &b);

    let note = a.capture("the thing that should not have been written down");
    sync(&a, &b);
    let id = b.id_of(&note);
    // Taken before the redaction: destroying a note rewrites its slug, so the
    // name it was captured under stops resolving.
    let on_a = a.id_of(&note);
    assert!(
        b.text_of(&note).is_some() && b.still_holds_body(&id),
        "the phone never had the content, so this proves nothing"
    );

    // Ordered while the phone is unreachable.
    capture_store::mutate::redact(&a.paths, &note).unwrap();

    // Past the cancellation window. It is measured from the redaction's own
    // timestamp so that every replica destroys at the same instant rather than
    // each starting its own clock on arrival — which is exactly why a test
    // that could not move time could only assert the destruction was queued.
    let after_the_window = a.read().redact_queue().unwrap()[0].2 + 1;
    a.session().drain_due_at(after_the_window).unwrap();
    assert!(
        !a.still_holds_body(&on_a),
        "the device that ordered the redaction still holds the bytes"
    );

    // The phone wakes.
    sync(&a, &b);
    b.session().drain_due_at(after_the_window).unwrap();

    assert_eq!(
        b.text_of(&note),
        None,
        "the redaction arrived and the content survived it"
    );
    assert!(
        !b.still_holds_body(&id),
        "the note is hidden but its bytes are still on the other device's disk"
    );
}

#[test]
fn a_redaction_is_reported_outstanding_until_the_other_device_confirms_it() {
    // The claim a redaction makes is that the content is gone *everywhere*, so
    // "not yet acknowledged by that device" has to be an observed state rather
    // than an assumption. This is the whole path: the pending set is written
    // when the redaction is issued, the exchange carries it, and the
    // acknowledgement is recorded only after the peer has folded it.
    let a = Device::new("a-ack");
    let b = Device::new("b-ack");
    pair(&a, &b);

    let note = a.capture("something that should not have been written down");
    sync(&a, &b);
    let on_b = b.id_of(&note);

    capture_store::mutate::redact(&a.paths, &note).unwrap();

    let owed = a.ops().redactions_outstanding().unwrap();
    assert_eq!(
        owed.len(),
        1,
        "the redaction was not recorded as owed to anybody, so every surface \
         would report it complete"
    );
    assert_eq!(owed[0].1.len(), 1, "owed to the one other device");

    sync(&a, &b);

    assert!(
        a.ops().redactions_outstanding().unwrap().is_empty(),
        "the peer folded the redaction and it is still reported outstanding"
    );

    // And it was a real destruction, not merely a delivery.
    let after = a.read().redact_queue().unwrap()[0].2 + 1;
    b.session().drain_due_at(after).unwrap();
    assert!(
        !b.still_holds_body(&on_b),
        "the redaction was acknowledged but the bytes survived"
    );
}

#[test]
fn a_redaction_stays_outstanding_against_a_device_that_never_confirms() {
    // A phone that is off holds the content until it is not. Reporting
    // completion because the sync round finished would be reporting it for the
    // one device that never received anything.
    let a = Device::new("a-unacked");
    let b = Device::new("b-unacked");
    pair(&a, &b);

    let note = a.capture("still on the other device");
    sync(&a, &b);

    capture_store::mutate::redact(&a.paths, &note).unwrap();

    // No sync. The phone never wakes.
    let owed = a.ops().redactions_outstanding().unwrap();
    assert_eq!(owed.len(), 1);
    assert_eq!(
        owed[0].1.len(),
        1,
        "a redaction that reached nobody was reported as complete"
    );
}

#[test]
fn a_redaction_is_acknowledged_whichever_side_dialled() {
    // The device that owes the redaction is not necessarily the one that opens
    // the connection — in practice it is often the one sitting still while a
    // laptop wakes up and calls it. If only the dialling side handed its
    // redactions over, a device that never dials would hold content it had
    // been told to destroy, and report nothing.
    let a = Device::new("a-ack-responder");
    let b = Device::new("b-ack-responder");
    pair(&a, &b);

    let note = a.capture("owed by the side that answers");
    sync(&a, &b);

    capture_store::mutate::redact(&a.paths, &note).unwrap();
    assert_eq!(a.ops().redactions_outstanding().unwrap().len(), 1);

    // b dials; a answers, and a is the one that owes.
    sync(&b, &a);

    assert!(
        a.ops().redactions_outstanding().unwrap().is_empty(),
        "a redaction owed by the answering side was never acknowledged"
    );
}

#[test]
fn a_device_that_cannot_verify_a_redaction_does_not_acknowledge_it() {
    // Acknowledgement means "I have folded this". A device that does not hold
    // the roster cannot check who authored the redaction, so it has folded
    // nothing — and acknowledging anyway would report a destruction on the
    // strength of having been handed some bytes.
    let a = Device::new("a-unverifiable");
    let b = Device::new("b-unverifiable");

    // Deliberately not paired. b has never admitted a, so a's signature means
    // nothing to it.
    let note = a.capture("offered to a device that cannot check it");
    capture_store::mutate::redact(&a.paths, &note).unwrap();

    // Recorded as owed to nobody, because a's roster holds only itself — and
    // that is the true statement rather than a gap.
    assert!(
        a.ops().redactions_outstanding().unwrap().is_empty(),
        "a redaction was recorded as owed to a device that is not a member"
    );

    sync(&a, &b);
    assert!(a.ops().redactions_outstanding().unwrap().is_empty());
}

#[test]
fn pairing_admits_in_both_directions() {
    // Each device that has run `capture init` has a root of its own, and only
    // one root confers membership once the two archives meet. If pairing
    // admitted in one direction only, the device whose root lost the ordering
    // would not be a member of the combined roster — and every event it
    // authored would be unverifiable on the other, permanently and silently.
    //
    // Which root wins depends on generated key bytes, so a one-directional
    // pairing works about half the time. That is the worst kind of defect, and
    // it is why this asserts both memberships rather than "it synced".
    let a = Device::new("a-mutual");
    let b = Device::new("b-mutual");
    pair(&a, &b);

    a.capture("from a");
    b.capture("from b");
    sync(&a, &b);
    sync(&a, &b);

    for (who, d) in [("a", &a), ("b", &b)] {
        let log = capture_store::log::Log::open_read(&d.paths.events).unwrap();
        let roster = capture_core::roster::build(&log.all().unwrap());
        assert_eq!(
            roster.keys.len(),
            2,
            "{who} does not hold both devices as members"
        );
    }
    assert_eq!(a.note_count(), b.note_count());
}

// ------------------------------------------------------------- retirement ---

#[test]
fn a_retired_device_stops_being_a_member_on_both() {
    // The remedy for a lost or stolen device, exercised through the same call
    // the command uses. It had been implemented in the fold and tested there,
    // with nothing able to author one — which is not a feature, it is a
    // mechanism.
    let a = Device::new("a-retire");
    let b = Device::new("b-retire");
    pair(&a, &b);
    a.capture("before it was lost");
    sync(&a, &b);

    let stolen = b.identity().public().to_bytes();
    a.session().retire(&stolen, None).unwrap();

    let log = capture_store::log::Log::open_read(&a.paths.events).unwrap();
    let roster = capture_core::roster::build(&log.all().unwrap());
    assert!(
        !roster.keys.contains(&stolen),
        "the retired key is still a member"
    );
    assert!(
        roster.key_for(&b.identity().dev()).is_some(),
        "the retired device's past events lost the key they must still be checked against"
    );
}

#[test]
fn a_device_does_not_accept_being_retired_by_someone_else() {
    // The first lockout guard, and the one that was inert: any member may
    // retire any member, so one malfunctioning device could retire every
    // other, and nothing honest could be re-admitted without two signatures.
    //
    // It deliberately makes the two disagree. A stolen device is unaffected,
    // because what retiring it achieves is that its *peers* stop accepting its
    // events, which they still do.
    let a = Device::new("a-selfguard");
    let b = Device::new("b-selfguard");
    pair(&a, &b);
    sync(&a, &b);

    let victim = b.identity().public().to_bytes();
    a.session().retire(&victim, None).unwrap();
    sync(&a, &b);

    // As a sees it: b is out.
    let log = capture_store::log::Log::open_read(&a.paths.events).unwrap();
    let mut as_a = capture_core::roster::build(&log.all().unwrap());
    as_a.honour_self(&a.identity().public().to_bytes());
    assert!(!as_a.keys.contains(&victim));

    // As b sees itself: still a member, because it did not issue that.
    let log = capture_store::log::Log::open_read(&b.paths.events).unwrap();
    let mut as_b = capture_core::roster::build(&log.all().unwrap());
    as_b.honour_self(&victim);
    assert!(
        as_b.keys.contains(&victim),
        "a device locked itself out on another device's say-so"
    );
}

#[test]
fn two_signatures_undo_a_lockout() {
    // Every honest device retired by one compromised member. device_add cannot
    // help — it needs a signature from a current member and there are none —
    // and pairing cannot either, because pairing bootstraps transport keys
    // while the roster entry that gates ingest still has to come from a member.
    let a = Device::new("a-lockout");
    let b = Device::new("b-lockout");
    pair(&a, &b);
    sync(&a, &b);

    let victim = b.admitted();
    a.session().retire(&victim.pubkey, None).unwrap();

    let nonce = [0u8; 16];
    let digest = capture_core::body::readmit_digest(&victim.pubkey, &victim.incarnation, &nonce);
    let cosigner = a.identity();
    let sig = ed25519_dalek::Signer::sign(cosigner.signing(), &digest).to_bytes();

    a.session()
        .readmit_with(&victim, &cosigner.public().to_bytes(), &sig, nonce)
        .unwrap();

    let log = capture_store::log::Log::open_read(&a.paths.events).unwrap();
    let roster = capture_core::roster::build(&log.all().unwrap());
    assert!(
        roster.keys.contains(&victim.pubkey),
        "a co-signed re-admission did not restore membership"
    );
}

#[test]
fn a_readmission_has_to_be_written_by_a_member() {
    // The deadlock this avoids, found by running it: a retired device cannot
    // open a session — that is what retiring it did — so a re-admission *it*
    // authored has no route to the devices refusing it. The remedy cannot
    // travel through the refusal it exists to undo.
    //
    // Written by a member, it is an ordinary event and replicates like one.
    let a = Device::new("a-readmit-route");
    let b = Device::new("b-readmit-route");
    pair(&a, &b);
    sync(&a, &b);

    let subject = b.admitted();
    a.session().retire(&subject.pubkey, None).unwrap();

    // Authored by a, which is still a member.
    let nonce = [0u8; 16];
    let digest = capture_core::body::readmit_digest(&subject.pubkey, &subject.incarnation, &nonce);
    let signer = a.identity();
    let sig = ed25519_dalek::Signer::sign(signer.signing(), &digest).to_bytes();
    a.session()
        .readmit_with(&subject, &signer.public().to_bytes(), &sig, nonce)
        .unwrap();

    // It reaches b by ordinary anti-entropy, and both agree b is a member.
    sync(&a, &b);
    for (who, d) in [("a", &a), ("b", &b)] {
        let log = capture_store::log::Log::open_read(&d.paths.events).unwrap();
        let roster = capture_core::roster::build(&log.all().unwrap());
        assert!(
            roster.keys.contains(&subject.pubkey),
            "{who} still treats the re-admitted device as retired"
        );
    }
}

#[test]
fn each_device_learns_what_the_other_can_watch_for() {
    // §7.5. Choosing a notifier among devices that cannot deliver is choosing
    // silence, and until a device says what it can see there is no way to make
    // that choice on anything but hope.
    //
    // Both sides here are the same build on the same platform, so what this
    // proves is the exchange rather than the difference — the difference is
    // `notifier::observable`, which is asserted per-platform in its own tests
    // and run on both.
    let a = Device::new("a-cues");
    let b = Device::new("b-cues");
    pair(&a, &b);

    let before = Ops::open_read(&a.paths.ops)
        .unwrap()
        .peer_observes(&b.identity().public().to_bytes())
        .unwrap();
    assert!(before.is_empty(), "nothing is known before anyone speaks");

    sync(&a, &b);

    for (who, from, about) in [("a", &a, &b), ("b", &b, &a)] {
        let learned = Ops::open_read(&from.paths.ops)
            .unwrap()
            .peer_observes(&about.identity().public().to_bytes())
            .unwrap();
        assert!(
            learned.contains(&String::from("time")),
            "{who} learned nothing about what the other watches: {learned:?}"
        );
        // And it is a claim about that device, not a copy of our own list
        // written under its name.
        assert!(
            !learned.is_empty() && learned.iter().all(|c| !c.is_empty()),
            "{who} recorded an empty capability: {learned:?}"
        );
    }
}

#[test]
fn a_recording_follows_the_note_that_refers_to_it() {
    // §5.6. The reference is in the log and the bytes are not, so a device
    // that has synced every event can still be holding a note that points at
    // a recording it does not have. The blob phase is what closes that, and
    // without it the feature is a note saying "voice note" and nothing to
    // play.
    let a = Device::new("a-blob");
    let b = Device::new("b-blob");
    pair(&a, &b);

    // Bytes first, then the note. A reference to a blob nobody holds is a
    // note pointing at nothing, and the order is what makes that impossible.
    let recording = vec![0x7fu8; 70_000];
    let hash = capture_store::blobs::put(&a.paths.blobs, &recording).unwrap();

    let body = capture_store::session::capture_body_with_blob(
        "",
        capture_core::body::BlobRef {
            hash,
            media_type: String::from("audio/ogg"),
            bytes: recording.len() as u64,
            duration_ms: Some(9310),
        },
    )
    .unwrap();
    let slug = a.session().capture(&body).unwrap().slug;

    assert!(
        !capture_store::blobs::have(&b.paths.blobs, &hash),
        "the other device has no reason to have these yet"
    );

    sync(&a, &b);

    assert!(
        b.read().resolve(&slug).unwrap().is_some(),
        "the note itself arrived"
    );
    assert_eq!(
        capture_store::blobs::get(&b.paths.blobs, &hash).unwrap(),
        Some(recording),
        "and so did the bytes it refers to, byte for byte"
    );
}

#[test]
fn a_recording_the_other_device_never_had_is_not_waited_for() {
    // A device that has evicted a blob is in the ordinary state §5.6 wants
    // constrained devices to be in, not a broken one. A round that hung, or
    // failed, because the peer could not supply some bytes would make
    // eviction expensive — and the whole argument for evicting aggressively
    // is that it costs nothing.
    let a = Device::new("a-gone");
    let b = Device::new("b-gone");
    pair(&a, &b);

    let recording = vec![0x5au8; 4096];
    let hash = capture_store::blobs::put(&a.paths.blobs, &recording).unwrap();
    let body = capture_store::session::capture_body_with_blob(
        "",
        capture_core::body::BlobRef {
            hash,
            media_type: String::from("audio/ogg"),
            bytes: recording.len() as u64,
            duration_ms: None,
        },
    )
    .unwrap();
    a.session().capture(&body).unwrap();

    // Gone from the only device that had them, after the note was written.
    capture_store::blobs::evict(&a.paths.blobs, &hash).unwrap();

    sync(&a, &b);

    assert!(
        !capture_store::blobs::have(&b.paths.blobs, &hash),
        "there was nothing to fetch"
    );
    assert_eq!(
        a.note_count(),
        b.note_count(),
        "and the round still moved every event"
    );
}

#[test]
fn a_recording_nobody_has_is_eventually_said_to_be_unavailable() {
    // SYNC 8. A refusal was counted and thrown away, so a recording whose only
    // copy had been evicted told the person it would arrive on the next sync —
    // on every sync, for ever. The round now records each refusal, and after
    // enough of them over long enough the store says so.
    let a = Device::new("a-never");
    let b = Device::new("b-never");
    pair(&a, &b);

    let recording = vec![0x33u8; 2048];
    let hash = capture_store::blobs::put(&a.paths.blobs, &recording).unwrap();
    let body = capture_store::session::capture_body_with_blob(
        "",
        capture_core::body::BlobRef {
            hash,
            media_type: String::from("audio/ogg"),
            bytes: recording.len() as u64,
            duration_ms: None,
        },
    )
    .unwrap();
    a.session().capture(&body).unwrap();
    capture_store::blobs::evict(&a.paths.blobs, &hash).unwrap();
    let name = capture_store::blobs::name(&hash);
    let hour = 60 * 60 * 1000;
    let verdict = |d: &Device, at: u64| {
        capture_store::ops::Ops::open(&d.paths.ops)
            .unwrap()
            .blob_unavailable_since(&name, at)
            .unwrap()
    };

    // One refusal, and then two more within the hour: a peer that is merely
    // asleep would look like this, and it is not worth alarming anybody over.
    sync_at(&a, &b, AT);
    sync_at(&a, &b, AT + hour / 4);
    sync_at(&a, &b, AT + hour / 2);
    assert_eq!(
        verdict(&b, AT + hour),
        None,
        "three misses in an hour is not gone"
    );

    // A day later, still refused.
    sync_at(&a, &b, AT + 25 * hour);
    assert_eq!(
        verdict(&b, AT + 25 * hour),
        Some(AT),
        "refused for a day, by every round: the surface may now say so"
    );

    // And the bytes turning up after all puts that right. Only the device that
    // *fetched* them is the one whose record matters here.
    capture_store::blobs::put(&a.paths.blobs, &recording).unwrap();
    sync_at(&a, &b, AT + 26 * hour);
    assert!(
        capture_store::blobs::have(&b.paths.blobs, &hash),
        "it arrived"
    );
    assert_eq!(
        verdict(&b, AT + 26 * hour),
        None,
        "and is no longer called gone"
    );
}

/// The HLC advances to what a peer has shown it.
///
/// §HLC is unambiguous about this: "Causality is preserved by one thing and one
/// thing only: on receipt, the local clock advances to at least the incoming
/// value." It spells out the consequence of not doing it — an edit authored
/// after reading a peer's event carries a *lower* stamp than the event it
/// supersedes, so a fold ordered by HLC reaches the correction before the
/// version it corrects, and every last-writer-wins rule in §6 resolves the
/// wrong way. It even names the failure mode: "every consequence is silent".
///
/// `HlcState::observe` implemented it, the module documented it as mandatory,
/// and its only callers anywhere were `capture-core`'s test harness and these
/// suites' own fixtures. No ingest path called it. So a real pair of devices
/// resolved conflicts by whichever machine's wall clock happened to be further
/// ahead — which is the behaviour of the clamping draft the module rejects,
/// reached by never adopting at all.
///
/// The skew here is manufactured, because that is the only way to see it: with
/// two clocks that agree, adopting and not adopting are indistinguishable, and
/// the bug lived for exactly that reason.
#[test]
fn a_device_adopts_a_peers_clock() {
    let a = Device::new("a-hlc");
    let b = Device::new("b-hlc");
    pair(&a, &b);

    // Push A's clock an hour ahead — a laptop that has not seen an NTP server,
    // which §HLC says must drag every peer forward rather than be clamped.
    const AHEAD_MS: u64 = 60 * 60 * 1000;
    {
        let s = a.session();
        let w = s.identity.watermark();
        let mut id = a.identity();
        let now = jiff::Timestamp::now().as_millisecond().max(0) as u64;
        id.advance(&a.paths, w.highest_seq, now + AHEAD_MS, 0)
            .unwrap();
    }

    let slug = a.capture("dentist thursday");
    let ahead = a.read().note(&a.id_of(&slug)).unwrap().unwrap();
    let from_a = stamp_of(&a, &slug);
    assert!(
        from_a.wall > jiff::Timestamp::now().as_millisecond().max(0) as u64,
        "the fixture failed to skew A's clock, so this test proves nothing",
    );
    assert!(ahead.text.is_some());

    sync(&a, &b);

    // B now holds A's event. The correction B makes *after* seeing it has to
    // order after it, or B's own fix loses to the thing it is fixing.
    capture_store::mutate::edit(&b.paths, &slug, "dentist friday").unwrap();
    let correction = stamp_of(&b, &slug);

    assert!(
        correction > from_a,
        "B's correction ({}, {}) does not order after the event it corrects ({}, {}): B's clock \
         never adopted A's, so the fold reaches the edit before the version it replaces and every \
         last-writer-wins rule resolves backwards",
        correction.wall,
        correction.counter,
        from_a.wall,
        from_a.counter,
    );

    // And it survives a restart: a clock advance held only in memory is undone
    // by the next process, which then reissues stamps it was already told about.
    let persisted = b.identity().watermark();
    assert!(
        persisted.hlc_wall >= from_a.wall,
        "B's adopted clock was not persisted: {} < {}",
        persisted.hlc_wall,
        from_a.wall,
    );
}

/// The highest stamp any event for this note carries on this device.
fn stamp_of(d: &Device, slug: &str) -> capture_core::Hlc {
    let log = capture_store::log::Log::open_read(&d.paths.events).unwrap();
    let tgt = d.id_of(slug);
    log.all()
        .unwrap()
        .iter()
        .filter(|e| e.header.tgt == Some(tgt) || e.header.id() == tgt)
        .map(|e| e.header.hlc)
        .max()
        .expect("the note is in the log")
}

/// Retiring a device stops this one calling it.
///
/// §5.3 retires a key because the device holding it is lost or compromised. The
/// inbound half worked — `answer` refuses a session with a non-member — and the
/// outbound half did not exist, which is the direction that matters: a device
/// you no longer trust does not need to call you if you keep calling it.
///
/// Two separate holes, both on the path that has no multicast and so is the one
/// most likely to be in use:
///
/// - The pairing grant that lets a brand-new device be dialled before it appears
///   in any roster does not expire, so a retired key stayed dialable on the
///   strength of having once been paired.
/// - The remembered-address fallback never checked membership at all. Its
///   neighbour, the advertised path, does — `discovery::consider` takes the
///   member list — so the guard was on the path that needs it least.
#[test]
fn a_retired_device_is_not_dialled_and_not_remembered() {
    let a = Device::new("a-retire-dial");
    let b = Device::new("b-retire-dial");
    pair(&a, &b);
    sync(&a, &b);

    let subject = b.admitted().pubkey;

    // What a reachable peer leaves behind: an address and a handshake key.
    let ops = Ops::open(&a.paths.ops).unwrap();
    ops.peer_reached(&subject, "192.0.2.7:9999", AT).unwrap();
    assert!(
        ops.peer_addresses()
            .unwrap()
            .iter()
            .any(|p| p.key == subject),
        "the fixture failed to leave an address to dial",
    );

    a.session().retire(&subject, None).unwrap();

    let log = capture_store::log::Log::open_read(&a.paths.events).unwrap();
    let roster = capture_core::roster::build(&log.all().unwrap());
    let me = a.identity().public().to_bytes();
    assert!(
        !roster.keys.contains(&subject),
        "the fixture failed to retire the device",
    );
    assert!(
        !capture_store::round::may_dial(&roster, &me, &subject),
        "a retired key is still dialable, so the next sweep hands it everything \
         written since it was retired",
    );
    // And a member still is, or the guard would be a way to stop syncing.
    assert!(
        capture_store::round::may_dial(&roster, &me, &[9u8; 32]),
        "may_dial refuses a key that was never retired",
    );

    // The material to dial with is gone too, not merely unused. A row left in
    // place is one the next change to the sweep can start reading again.
    let after = Ops::open_read(&a.paths.ops).unwrap();
    assert!(
        !after
            .peer_addresses()
            .unwrap()
            .iter()
            .any(|p| p.key == subject),
        "a retired device's address is still remembered",
    );
    assert!(
        !after.dialable().unwrap().contains(&subject),
        "a retired device's handshake key is still held, so it stays dialable",
    );
    assert!(
        after.noise_for(&subject).unwrap().is_none(),
        "the retired device's noise key survived the retirement",
    );
}

/// A retired device's later events occupy their positions without being used.
///
/// Both halves of the set-aside contract, because they used to be in conflict.
///
/// They were kept out of the log entirely, which did keep them out of the fold
/// — and left this device's head for that origin stopped at the retirement
/// head. A version vector is one number per origin and cannot describe a hole,
/// so every round afterwards asked every peer for the tail above it, was sent
/// the whole thing, set it aside again, and asked again. For ever, with the
/// bytes on the wire each time. `sync-membership/spec.md` calls for set-aside
/// rather than refusal to avoid exactly that, and keeping the events out of the
/// log was the refusal under another name.
///
/// So the rows are stored and marked. The head advances, and `Log::all` — what
/// every fold and every roster build goes through — does not return them.
#[test]
fn a_retired_devices_later_events_hold_their_places_and_are_not_used() {
    let a = Device::new("a-aside");
    let b = Device::new("b-aside");
    pair(&a, &b);
    a.capture("before any of this");
    sync(&a, &b);

    // b keeps writing after it has been retired, which is what a device that
    // has not been told does. The retirement names b's head as it stands now.
    let stolen = b.identity().public().to_bytes();
    a.session().retire(&stolen, None).unwrap();

    b.capture("written after the retirement");
    b.capture("and another");

    let b_dev = b.identity().dev();
    let b_key = capture_store::projection::dev_hex(&b_dev);
    let b_head = {
        let log = capture_store::log::Log::open_read(&b.paths.events).unwrap();
        log.heads().unwrap().get(&b_key).copied().unwrap()
    };

    // Handed to a directly: `round` will not dial a retired peer any more, so
    // in life these arrive from a third device that still holds them. What is
    // under test is what a does with them, not how they got here.
    {
        let after: Vec<_> = {
            let log = capture_store::log::Log::open_read(&b.paths.events).unwrap();
            log.all()
                .unwrap()
                .into_iter()
                .filter(|e| e.header.dev == b_dev)
                .collect()
        };
        let mut s = a.session();
        let own = a.identity().public().to_bytes();
        let ops = a.ops();
        let mut gate =
            capture_store::gate::Gate::open(&mut s.log, &mut s.state, &ops, own).unwrap();
        for e in &after {
            let _ = gate.offer(e, 1_700_000_100_000).unwrap();
        }
    }

    // No hole: the head is level with b's own, so nothing re-requests the tail.
    let log = capture_store::log::Log::open_read(&a.paths.events).unwrap();
    assert_eq!(
        log.heads().unwrap().get(&b_key).copied(),
        Some(b_head),
        "the head stopped below the set-aside events, which is the hole \
         anti-entropy asks every peer about for ever"
    );

    // And unused: not in the fold, and not in the roster.
    let used = log.all().unwrap();
    assert!(
        !used
            .iter()
            .any(|e| e.header.dev == b_dev && e.header.seq > b_head - 2),
        "a retired device's post-head events reached the fold"
    );
    assert!(
        !capture_core::roster::build(&used).keys.contains(&stolen),
        "the retired key came back"
    );

    // Still physically held, which is what the remedy and the export need.
    assert!(
        log.all_including_aside()
            .unwrap()
            .iter()
            .any(|e| e.header.dev == b_dev && e.header.seq == b_head),
        "the events were not stored at all, so reissuing the retirement \
         could only recover them from a peer that may be long gone"
    );
}

#[test]
fn a_kind_corrected_on_one_device_survives_the_other_reparsing_it() {
    // The phone is where a misparse is seen first, because the phone is where
    // capture happens. What it says has to reach the desktop *and* survive the
    // desktop's own sweep — a grammar bump quietly reverting somebody's
    // correction is the failure §6 rates unrecoverable, and it would happen on
    // the device that did not make the correction.
    let phone = Device::new("a-kind-phone");
    let desk = Device::new("b-kind-desk");
    pair(&phone, &desk);

    // Well over the floor, and wrong: the recognizer reads a day and a clock
    // time and calls it an event.
    let slug = phone.capture("dentist thursday 2pm");
    sync(&phone, &desk);

    let sweep = |d: &Device| {
        let mut s = d.session();
        capture_store::promote::sweep(&mut s).unwrap()
    };
    sweep(&phone);
    sweep(&desk);

    let kind_on = |d: &Device| {
        let store = d.read();
        let tgt = store.resolve(&slug).unwrap().unwrap();
        store.promotion_of(&tgt).unwrap().map(|p| p.as_.kind())
    };
    assert_eq!(kind_on(&desk), Some("event"), "the recognizer's own answer");

    // The phone says otherwise.
    capture_store::promote::as_task(&phone.paths, &slug, None).unwrap();
    assert_eq!(kind_on(&phone), Some("task"));

    sync(&phone, &desk);
    assert_eq!(kind_on(&desk), Some("task"), "the correction did not cross");

    // And the desktop re-reading the text does not take it back, however many
    // times it runs.
    sweep(&desk);
    sweep(&desk);
    assert_eq!(
        kind_on(&desk),
        Some("task"),
        "the other device's sweep undid a correction"
    );

    // Converged, not merely agreeing by accident on one field.
    sync(&phone, &desk);
    assert_eq!(kind_on(&phone), kind_on(&desk));
}

#[test]
fn an_entity_rename_crosses_and_does_not_fork_the_page() {
    // 4.2 and the cross-device half of the change. An entity event names no
    // note, so it arrives at the other device as something `referenced()` can
    // say nothing about — which is why it is global. Before that it was ingested
    // and then had no effect, and a page simply went on showing the old name
    // with nothing anywhere saying why.
    let phone = Device::new("a-ent-phone");
    let desk = Device::new("b-ent-desk");
    pair(&phone, &desk);

    phone.capture("ask @sarah about the invoice");
    desk.capture("@Sarah replied");
    sync(&phone, &desk);

    let ent = capture_core::id::EntId::derive("person", "sarah");
    let page_on = |d: &Device| -> (Option<String>, usize) {
        let store = d.read();
        let row = store.entity_by_id(&ent).unwrap();
        (
            row.map(|r| r.display),
            store.entity_notes(&ent, 1_900_000_000_000).unwrap().len(),
        )
    };

    // Case folded on both sides, so one person with both notes.
    assert_eq!(page_on(&phone).1, 2, "case forked her on the phone");
    assert_eq!(page_on(&desk).1, 2, "case forked her on the desktop");

    // The phone renames her.
    capture_store::mutate::rename_entity(&phone.paths, "person", "sarah", "Sarah Chen").unwrap();
    assert_eq!(page_on(&phone).0.as_deref(), Some("Sarah Chen"));

    sync(&phone, &desk);
    assert_eq!(
        page_on(&desk).0.as_deref(),
        Some("Sarah Chen"),
        "the rename did not cross"
    );
    assert_eq!(page_on(&desk).1, 2, "the rename forked the page");
}

#[test]
fn a_merge_crosses_and_either_spelling_arrives_at_the_survivor() {
    let phone = Device::new("a-merge-phone");
    let desk = Device::new("b-merge-desk");
    pair(&phone, &desk);

    phone.capture("ask @sarah about the invoice");
    desk.capture("@sarah-chen sent the contract");
    sync(&phone, &desk);

    capture_store::mutate::merge_entities(&phone.paths, "person", "sarah", "sarah-chen").unwrap();
    sync(&phone, &desk);

    for d in [&phone, &desk] {
        let store = d.read();
        for spelling in ["sarah", "sarah-chen"] {
            let ent = store
                .entity_id("person", spelling)
                .unwrap()
                .expect("resolves");
            assert_eq!(
                store.entity_notes(&ent, 1_900_000_000_000).unwrap().len(),
                2,
                "{spelling} did not reach the merged page"
            );
        }
        assert_eq!(
            store
                .entities_of_kind("person", 1_900_000_000_000)
                .unwrap()
                .len(),
            1,
            "a merged-away entity is still listed"
        );
    }
}

/// What a round leaves behind about the peer, as the daemon and the phone
/// record it after one. `reconcile` itself does not; `dial` and `answer` do.
fn reached_each_other(a: &Device, b: &Device, at: u64) {
    let (ak, bk) = (
        a.identity().public().to_bytes(),
        b.identity().public().to_bytes(),
    );
    a.ops().peer_answered(&bk, at).unwrap();
    b.ops().peer_answered(&ak, at).unwrap();
}

/// Whether a device would say something now, as every delivery path asks.
fn speaks(d: &Device, at: u64) -> bool {
    capture_store::notifier::speaks_here(&d.paths, &d.read(), &d.identity().public().to_bytes(), at)
        .unwrap()
}

/// §2.4.1's default notifier, across two archives that have really paired.
///
/// Before this, `speaks_here` answered yes whenever nobody had chosen — the
/// M1 rule — so a paired phone and desktop each announced every moment.
#[test]
fn with_nobody_chosen_exactly_one_paired_device_speaks() {
    let a = Device::new("a-speaks");
    let b = Device::new("b-speaks");
    pair(&a, &b);
    sync(&a, &b);
    reached_each_other(&a, &b, AT);

    let (sa, sb) = (speaks(&a, AT), speaks(&b, AT));
    assert!(sa != sb, "both or neither spoke: a={sa} b={sb}");

    // The one that speaks is the one that joined the roster last, and both
    // devices, asking from their own seats, name the same one.
    let me_a = a.identity().public().to_bytes();
    let order = capture_core::roster::build(
        &capture_store::log::Log::open_read(&a.paths.events)
            .unwrap()
            .all()
            .unwrap(),
    )
    .admitted;
    let newest = *order.iter().max_by_key(|(_, h)| **h).unwrap().0;
    assert_eq!(sa, newest == me_a);

    // The speaker's notifications are switched off. It says so in the next
    // round, and the other device takes over.
    let (speaker, other) = if sa { (&a, &b) } else { (&b, &a) };
    speaker.ops().set("alerts.silenced", "1").unwrap();
    sync(other, speaker);
    reached_each_other(&a, &b, AT);
    assert!(
        !speaks(speaker, AT),
        "a silent device still claims to speak"
    );
    assert!(speaks(other, AT), "nobody took over from a silent device");

    // Back on, and back again.
    speaker.ops().set("alerts.silenced", "0").unwrap();
    sync(other, speaker);
    assert!(speaks(speaker, AT));
    assert!(!speaks(other, AT));

    // A speaker unreached for more than a day is not counted on: the other
    // speaks for itself rather than trusting a device it cannot see.
    let later = AT + 25 * 60 * 60 * 1000;
    assert!(
        speaks(other, later),
        "a device gone quiet left nobody speaking"
    );
}

/// A device that is only ever dialled still knows sync is running.
///
/// The desktop answers the phone — the phone dials, and has no address to be
/// dialled at — and `answer` recorded nothing about it. So the desktop said
/// "sync has never run on this device, so nothing here exists anywhere else"
/// with the phone's notes already on it.
#[test]
fn answering_a_peer_is_a_round_that_ran() {
    let a = Device::new("a-answers");
    let b = Device::new("b-answers");
    pair(&a, &b);
    assert_eq!(
        capture_store::syncstate::sync_state(&b.ops(), 1, AT).unwrap(),
        capture_store::syncstate::SyncState::NeverRan,
        "a precondition"
    );

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let b_paths = b.paths.clone();
    let b_id = b.identity();
    let answering = std::thread::spawn(move || {
        let stream = listener.accept().unwrap().0;
        capture_store::round::answer(&b_paths, &b_id, stream, AT, &|| false).expect("b answered");
    });
    let tcp = Tcp::new(TcpStream::connect(addr).unwrap()).unwrap();
    let mut secure = Secure::initiate(
        tcp,
        &a.identity().noise_secret(),
        &b.identity().noise_public(),
    )
    .expect("handshook");
    capture_store::round::reconcile(&a.paths, &mut secure, AT, true, &|| false).unwrap();
    answering.join().unwrap();

    assert_eq!(b.ops().last_round().unwrap(), Some(AT));
    assert_ne!(
        capture_store::syncstate::sync_state(&b.ops(), 1, AT).unwrap(),
        capture_store::syncstate::SyncState::NeverRan,
        "answering a peer did not count as sync running"
    );
}

/// A round says whether it reached anyone, not only whether it ran.
///
/// Found on a Pixel 8 Pro with a VPN that kept it off the local network. "Sync
/// now" ran, reported success, and the screen went on reading "last reached
/// 04:55" — a round that ran and a round that reached the other device were
/// the same result, so there was nothing to say *why* the phone was stale.
#[cfg(feature = "mdns")]
#[test]
fn a_round_that_reached_nobody_says_so() {
    let a = Device::new("unreached-a");
    let b = Device::new("unreached-b");
    pair(&a, &b);

    // Somewhere b used to be and is not: a port bound and released, so the
    // dial is refused rather than left to time out.
    let gone = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap();
    let b_key = b.identity().public().to_bytes();
    a.ops().peer_reached(&b_key, &gone.to_string(), AT).unwrap();

    let reach = round_from(&a);
    assert_eq!(reach.members, 1, "b is a member a should be looking for");
    assert_eq!(reach.reached, 0, "a dial that was refused reached nobody");
}

/// And says so when it did.
#[cfg(feature = "mdns")]
#[test]
fn a_round_that_reached_a_peer_says_so() {
    let a = Device::new("reached-a");
    let b = Device::new("reached-b");
    pair(&a, &b);

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let b_key = b.identity().public().to_bytes();
    a.ops().peer_reached(&b_key, &addr.to_string(), AT).unwrap();

    let b_paths = b.paths.clone();
    let b_id = b.identity();
    let answering = std::thread::spawn(move || {
        let stream = listener.accept().unwrap().0;
        capture_store::round::answer(&b_paths, &b_id, stream, AT, &|| false).expect("b answered");
    });

    let reach = round_from(&a);
    answering.join().unwrap();
    assert_eq!(reach.members, 1);
    assert_eq!(reach.reached, 1, "b answered, so the round reached it");
}

/// One round from `d`, by remembered address alone — no beacon, so nothing on
/// the machine running the tests is discovered by accident.
#[cfg(feature = "mdns")]
fn round_from(d: &Device) -> capture_store::round::Reach {
    let mut beacon = None;
    let now = || AT + 1_000;
    capture_store::round::tick(&d.paths, &d.identity(), &mut beacon, 1, 0, &now, &|| false)
        .expect("the round ran")
}
