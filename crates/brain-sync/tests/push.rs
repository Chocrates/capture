//! What a push promises, and what it does not.

use brain_core::testing;
use brain_sync::push::{Delivered, ack, push, push_redact};
use brain_sync::session::Session;
use brain_sync::transport::Pair;

fn sessions(
    pair: &Pair,
) -> (
    Session<brain_sync::transport::End>,
    Session<brain_sync::transport::End>,
) {
    let (ea, eb) = pair.ends();
    let responder = std::thread::spawn(move || Session::accept_paired(eb, &[9; 32]));
    let a = Session::initiate_paired(ea, &[9; 32]).expect("session");
    let b = responder.join().unwrap().expect("session");
    (a, b)
}

fn an_event() -> brain_core::envelope::Envelope {
    let mut o = testing::Origin::new(1);
    o.roster_root()
}

#[test]
fn a_pushed_event_reaches_the_peer() {
    let pair = Pair::new();
    let (mut a, mut b) = sessions(&pair);
    let e = an_event();

    assert_eq!(push(&mut a, &e), Delivered::Yes);

    let frame = b.recv().unwrap().expect("something arrived");
    match minicbor::decode(&frame).unwrap() {
        brain_sync::message::Message::Event(got) => assert_eq!(got.header.id(), e.header.id()),
        other => panic!("expected an event, got {other:?}"),
    }
}

#[test]
fn a_failed_push_is_not_an_error() {
    // Anything that treated it as one would be a thing that depends on
    // delivery having happened — a bug that only appears when the network is
    // bad, which is when it is hardest to notice.
    let pair = Pair::new();
    let (mut a, _b) = sessions(&pair);
    pair.close();

    assert_eq!(push(&mut a, &an_event()), Delivered::No);
}

#[test]
fn a_redaction_waits_to_be_told_it_was_folded() {
    let pair = Pair::new();
    let (mut a, mut b) = sessions(&pair);
    let e = an_event();
    let id = e.header.id();

    let peer = std::thread::spawn(move || {
        let frame = b.recv_blocking().unwrap();
        let arrived = match minicbor::decode(&frame).unwrap() {
            brain_sync::message::Message::Redact(env) => env,
            other => panic!("expected a redaction, got {other:?}"),
        };
        // Folded, then acknowledged. Never the other way round.
        ack(&mut b, &arrived, true).unwrap();
        b
    });

    assert!(push_redact(&mut a, &e).unwrap());
    let _b = peer.join().unwrap();
    assert_eq!(id, e.header.id());
}

#[test]
fn a_redaction_that_was_not_folded_is_not_acknowledged() {
    // Acknowledging on receipt would report a destruction that has not
    // happened — the one lie this protocol must not tell, because the whole
    // surface above it presents "gone everywhere" as fact.
    let pair = Pair::new();
    let (mut a, mut b) = sessions(&pair);
    let e = an_event();

    let peer = std::thread::spawn(move || {
        let frame = b.recv_blocking().unwrap();
        let arrived = match minicbor::decode(&frame).unwrap() {
            brain_sync::message::Message::Redact(env) => env,
            other => panic!("expected a redaction, got {other:?}"),
        };
        ack(&mut b, &arrived, false).unwrap();
        // The sender is still waiting. Ending the session is what a peer that
        // cannot fold looks like from the other side.
        drop(b);
    });

    // No acknowledgement, so this fails rather than returning true.
    let outcome = push_redact(&mut a, &e);
    peer.join().unwrap();
    assert!(
        outcome.is_err(),
        "an unfolded redaction was reported as acknowledged"
    );
}

#[test]
fn an_acknowledgement_for_another_redaction_does_not_count() {
    // Two redactions can be outstanding at once, and matching on "an ack
    // arrived" rather than "this ack arrived" would mark both complete when
    // one landed.
    let pair = Pair::new();
    let (mut a, mut b) = sessions(&pair);
    let mine = an_event();
    let mut other = testing::Origin::new(7);
    let unrelated = other.roster_root();

    let peer = std::thread::spawn(move || {
        let _ = b.recv_blocking().unwrap();
        ack(&mut b, &unrelated, true).unwrap();
        ack(&mut b, &mine, true).unwrap();
        b
    });

    let e = an_event();
    assert!(push_redact(&mut a, &e).unwrap());
    let _b = peer.join().unwrap();
}
