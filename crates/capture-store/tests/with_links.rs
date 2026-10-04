//! Redacting a note and the notes that copied its name.
//!
//! §1.1 limit 5: "Text that other notes copied is not reachable. … §4.3
//! therefore forbids deriving slugs from sensitive-looking captures and
//! provides `redact --with-links`, which redacts inbound linking text alongside
//! the target." §4.3 names it again. The string appeared nowhere in the
//! workspace.
//!
//! The hole is real: a note's slug comes from its opening words, and a note
//! linking to it carries that slug inside its own verbatim text — and often in
//! its own slug too. Destroying the target re-derives its name from its id, so
//! the name stops carrying the words, and the copies already written elsewhere
//! are untouched.

mod common;
use common::body_survives;

use capture_store::StorePaths;
use capture_store::session::Session;

struct Scratch(std::path::PathBuf);

impl Scratch {
    fn new(tag: &str) -> Scratch {
        let p = std::env::temp_dir().join(format!(
            "capture-wl-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        capture_store::store::create(&StorePaths::under(p.clone()), 1_700_000_000_000).unwrap();
        Scratch(p)
    }
    fn paths(&self) -> StorePaths {
        StorePaths::under(self.0.clone())
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

const PAST_THE_WINDOW: u64 = 2_000_000_000_000;

fn capture(paths: &StorePaths, text: &str) -> String {
    let mut s = Session::open_for_capture(paths.clone()).unwrap();
    let body = capture_store::session::capture_body(text).unwrap();
    s.capture(&body).unwrap().slug
}

fn tgt_of(paths: &StorePaths, slug: &str) -> capture_core::EventId {
    capture_store::projection::current(paths)
        .unwrap()
        .resolve(slug)
        .unwrap()
        .expect("the note is named")
}

/// The targets the command would name: the note, then what links to it.
fn with_links(paths: &StorePaths, slug: &str) -> Vec<capture_core::EventId> {
    let tgt = tgt_of(paths, slug);
    let mut out = vec![tgt];
    for b in capture_store::projection::current(paths)
        .unwrap()
        .links_to(&tgt)
        .unwrap()
    {
        if !out.contains(&b.tgt) {
            out.push(b.tgt);
        }
    }
    out
}

fn drain(paths: &StorePaths) {
    let mut s = Session::open(paths.clone()).unwrap();
    s.drain_due_at(PAST_THE_WINDOW).unwrap();
}

#[test]
fn the_target_and_what_links_to_it_both_go() {
    let sc = Scratch::new("both");
    let p = sc.paths();
    let target = capture(&p, "the vault passphrase is swordfish");
    capture_store::promote::settle(&p).unwrap();
    let linking = capture(&p, &format!("see [[{target}]] for the details"));
    capture_store::promote::settle(&p).unwrap();

    // Resolved before the destruction: a redacted note's slug is re-derived
    // from its id, so afterwards neither is findable by the name it had — which
    // is the half of §1.1 limit 5 that already worked.
    let linking_tgt = tgt_of(&p, &linking);
    let tgts = with_links(&p, &target);
    assert_eq!(tgts.len(), 2, "the backlink was not found: {tgts:?}");
    assert!(tgts.contains(&linking_tgt));

    let mut s = Session::open(p.clone()).unwrap();
    s.redact_all(&tgts).unwrap();
    drop(s);
    drain(&p);

    assert!(!body_survives(&p, &tgts[0]), "the target survived");
    assert!(
        !body_survives(&p, &linking_tgt),
        "the note carrying its name survived"
    );
}

#[test]
fn a_second_hop_is_not_followed() {
    // A note linking to a note linking to the target copied the *middle*
    // note's name, not this one's. Following further lets one flag walk an
    // arbitrary distance through the archive.
    let sc = Scratch::new("hop");
    let p = sc.paths();
    let target = capture(&p, "the vault passphrase is swordfish");
    capture_store::promote::settle(&p).unwrap();
    let middle = capture(&p, &format!("see [[{target}]] for the details"));
    capture_store::promote::settle(&p).unwrap();
    let far = capture(&p, &format!("and [[{middle}]] explains it"));
    capture_store::promote::settle(&p).unwrap();

    let far_tgt = tgt_of(&p, &far);
    let tgts = with_links(&p, &target);
    assert!(
        !tgts.contains(&far_tgt),
        "a second-hop note was named for destruction"
    );

    let mut s = Session::open(p.clone()).unwrap();
    s.redact_all(&tgts).unwrap();
    drop(s);
    drain(&p);

    assert!(
        body_survives(&p, &far_tgt),
        "a second-hop note was destroyed"
    );
}

#[test]
fn without_the_flag_only_the_target_goes() {
    let sc = Scratch::new("alone");
    let p = sc.paths();
    let target = capture(&p, "the vault passphrase is swordfish");
    capture_store::promote::settle(&p).unwrap();
    let linking = capture(&p, &format!("see [[{target}]] for the details"));
    capture_store::promote::settle(&p).unwrap();

    let linking_tgt = tgt_of(&p, &linking);
    let mut s = Session::open(p.clone()).unwrap();
    s.redact(tgt_of(&p, &target)).unwrap();
    drop(s);
    drain(&p);

    assert!(
        body_survives(&p, &linking_tgt),
        "an ordinary redaction took a linking note with it"
    );
}

#[test]
fn more_than_the_cap_is_refused() {
    // Redacting the ones that fit would leave the rest holding the words the
    // whole operation exists to remove, while reporting success.
    let sc = Scratch::new("overcap");
    let p = sc.paths();
    let cap = capture_core::body::REDACT_TARGET_CAP;
    let tgts: Vec<_> = (0..cap + 1)
        .map(|i| {
            let slug = capture(&p, &format!("note {i}"));
            tgt_of(&p, &slug)
        })
        .collect();
    capture_store::promote::settle(&p).unwrap();

    let mut s = Session::open(p.clone()).unwrap();
    let e = s
        .redact_all(&tgts)
        .expect_err("more than the cap must be refused");
    drop(s);

    assert!(
        format!("{e}").contains("at most"),
        "the refusal should say what the limit is: {e}"
    );
    for t in &tgts {
        assert!(body_survives(&p, t), "something was destroyed anyway");
    }
}
