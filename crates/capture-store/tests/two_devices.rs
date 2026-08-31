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

/// Run one round of anti-entropy between two stores over a real socket.
fn sync(a: &Device, b: &Device) {
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
        capture_store::round::reconcile(&b_paths, &mut secure).expect("the responder finished")
    });

    let tcp = Tcp::new(TcpStream::connect(addr).unwrap()).unwrap();
    let mut secure =
        Secure::initiate(tcp, &a_id.noise_secret(), &b_id.noise_public()).expect("handshook");
    capture_store::round::reconcile(&a.paths, &mut secure).expect("the initiator finished");

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
