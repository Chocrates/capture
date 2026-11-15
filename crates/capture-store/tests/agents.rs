//! An agent keeps notes through the CLI (an-agent-keeps-notes-through-the-cli).
//!
//! An agent is its own origin: its writes are signed by its own key, named in
//! history, kept out of everything that puts notes in front of the person, and
//! refused outright when they look like a credential.

#![allow(
    clippy::disallowed_methods,
    clippy::disallowed_types,
    reason = "review and search are asked as of the real now"
)]

use capture_store::session::Session;
use capture_store::{Error, StorePaths, agents, mutate, pages};
use proptest::prelude::*;

struct Scratch(std::path::PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let p = std::env::temp_dir().join(format!("capture-agents-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        // With its roster root in the log, as `capture init` writes it: an
        // agent is admitted by a member, and without a root there is none.
        let paths = StorePaths::under(p.clone());
        let fresh = capture_store::store::create(&paths, 1_700_000_000_000).unwrap();
        let mut log = capture_store::log::Log::open(&paths.events).unwrap();
        log.append(&fresh.root).unwrap();
        log.append(&fresh.roster_key).unwrap();
        let (wall, counter) = fresh.clock.persist();
        let mut identity = fresh.identity;
        identity
            .advance(&paths, fresh.roster_key.header.seq, wall, counter)
            .unwrap();
        Scratch(p)
    }
    fn person(&self) -> StorePaths {
        StorePaths::under(self.0.clone())
    }
    fn agent(&self, name: &str) -> StorePaths {
        self.person().acting_as(Some(name.to_string()))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn capture(paths: &StorePaths, text: &str) -> String {
    let body = capture_store::session::capture_body(text).unwrap();
    match capture_store::session::commit_body(paths, &body).unwrap() {
        capture_store::session::Committed::Stored(slug) => slug,
        capture_store::session::Committed::Spooled => panic!("spooled"),
    }
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

fn slugs(rows: Vec<capture_store::projection::NoteRow>) -> Vec<String> {
    rows.into_iter().map(|r| r.slug).collect()
}

fn origins_of(paths: &StorePaths, name: &str) -> Vec<capture_core::Dev> {
    let store = capture_store::projection::current(paths).unwrap();
    let tgt = store.resolve(name).unwrap().unwrap();
    let log = capture_store::log::Log::open_read(&paths.events).unwrap();
    log.all()
        .unwrap()
        .into_iter()
        .filter(|e| e.header.tgt == Some(tgt) || e.header.id() == tgt)
        .map(|e| e.header.dev)
        .collect()
}

fn agent_dev(paths: &StorePaths, name: &str) -> capture_core::Dev {
    agents::load(paths, name).unwrap().dev()
}

#[test]
fn an_agents_page_is_signed_by_the_agent_and_says_so() {
    let s = Scratch::new("signed");
    agents::add(&s.person(), "claude").unwrap();
    let made = pages::create(&s.agent("claude"), "Build notes", None).unwrap();
    let name = made.name.clone().unwrap();
    mutate::append(
        &s.agent("claude"),
        &name,
        "cargo test is slow on a cold cache",
    )
    .unwrap();

    let bot = agent_dev(&s.person(), "claude");
    let origins = origins_of(&s.person(), &name);
    assert!(origins.len() >= 3, "capture, page, edit: {origins:?}");
    assert!(
        origins.iter().all(|d| *d == bot),
        "every event is the agent's"
    );

    let store = capture_store::projection::current(&s.person()).unwrap();
    let tgt = store.resolve(&name).unwrap().unwrap();
    assert_eq!(
        store.note(&tgt).unwrap().unwrap().by.as_deref(),
        Some("claude")
    );
}

#[test]
fn the_persons_own_writes_are_still_the_persons() {
    let s = Scratch::new("person");
    agents::add(&s.person(), "claude").unwrap();
    let slug = capture(&s.person(), "my own thought");
    let bot = agent_dev(&s.person(), "claude");
    assert!(origins_of(&s.person(), &slug).iter().all(|d| *d != bot));
    let store = capture_store::projection::current(&s.person()).unwrap();
    let tgt = store.resolve(&slug).unwrap().unwrap();
    assert_eq!(store.note(&tgt).unwrap().unwrap().by, None);
}

#[test]
fn fifty_pages_add_nothing_to_look_at() {
    let s = Scratch::new("fifty");
    agents::add(&s.person(), "claude").unwrap();
    let mine = capture(&s.person(), "buy oat milk");
    for i in 0..50 {
        pages::create(
            &s.agent("claude"),
            &format!("Finding {i} about zebrafish"),
            None,
        )
        .unwrap();
    }
    // A capture, too, which on its own would be read for a date and a task.
    let theirs = capture(&s.agent("claude"), "call the dentist tomorrow at 9");

    let _ = Session::open(s.person()).unwrap(); // the sweep runs on open
    let store = capture_store::projection::current(&s.person()).unwrap();
    assert_eq!(slugs(store.timeline(500).unwrap()), vec![mine.clone()]);
    let t = now();
    for tier in [
        store.review_never_engaged(t, 500).unwrap(),
        store.review_by_context(t, 500).unwrap(),
        store.review_forward(t, 500).unwrap(),
    ] {
        assert!(
            tier.iter().all(|r| r.by.is_none()),
            "an agent's note in review"
        );
    }
    assert_eq!(
        store.unread_count(capture_parse::GRAMMAR_VERSION).unwrap(),
        0,
        "nothing waits to be settled"
    );
    let tgt = store.resolve(&theirs).unwrap().unwrap();
    assert!(
        store.promotion_of(&tgt).unwrap().is_none(),
        "an agent's text is not read for a task"
    );
    assert!(
        store.occurrences_of(&tgt).unwrap().is_empty(),
        "and nothing rings"
    );

    // Found, and each says who wrote it.
    let found = store.search("zebrafish", t as u64).unwrap();
    assert_eq!(found.len(), 50);
    for id in found {
        assert_eq!(
            store.note(&id).unwrap().unwrap().by.as_deref(),
            Some("claude")
        );
    }
}

#[test]
fn an_agents_edit_to_the_persons_page_stays_the_persons_page() {
    let s = Scratch::new("theirs-mine");
    agents::add(&s.person(), "claude").unwrap();
    let page = pages::create(&s.person(), "Packing", None)
        .unwrap()
        .name
        .unwrap();
    mutate::edit(&s.person(), &page, "Packing\n- [ ] passport").unwrap();
    let before = slugs(
        capture_store::projection::current(&s.person())
            .unwrap()
            .timeline(50)
            .unwrap(),
    );

    mutate::append(&s.agent("claude"), &page, "- [ ] adapter").unwrap();

    let store = capture_store::projection::current(&s.person()).unwrap();
    let tgt = store.resolve(&page).unwrap().unwrap();
    let row = store.note(&tgt).unwrap().unwrap();
    assert_eq!(
        row.text.as_deref(),
        Some("Packing\n- [ ] passport\n- [ ] adapter")
    );
    assert_eq!(
        row.by, None,
        "the person wrote the page; the agent added to it"
    );
    assert_eq!(
        slugs(store.timeline(50).unwrap()),
        before,
        "the page did not move"
    );
    let bot = agent_dev(&s.person(), "claude");
    assert_eq!(
        origins_of(&s.person(), &page).last(),
        Some(&bot),
        "the version is attributed"
    );
}

#[test]
fn an_agent_cannot_store_a_credential_and_nothing_is_written() {
    let s = Scratch::new("credential");
    agents::add(&s.person(), "claude").unwrap();
    let page = pages::create(&s.agent("claude"), "Deploy", None)
        .unwrap()
        .name
        .unwrap();
    let log = capture_store::log::Log::open_read(&s.person().events).unwrap();
    let before = log.count().unwrap();

    let refused = mutate::append(
        &s.agent("claude"),
        &page,
        "API key: sk-abc123def456ghi789jkl012",
    );
    assert!(matches!(refused, Err(Error::Refused(_))), "{refused:?}");
    let refused = mutate::edit(&s.agent("claude"), &page, "password: hunter2hunter2hunter2");
    assert!(matches!(refused, Err(Error::Refused(_))), "{refused:?}");
    let body =
        capture_store::session::capture_body("token ghp_a1b2c3d4e5f6g7h8i9j0k1l2m3n4o5p6q7r8")
            .unwrap();
    let refused = capture_store::session::commit_body(&s.agent("claude"), &body);
    assert!(
        matches!(refused, Err(Error::Refused(_))),
        "not spooled either"
    );

    let log = capture_store::log::Log::open_read(&s.person().events).unwrap();
    assert_eq!(log.count().unwrap(), before);
    assert!(!capture_store::spool::in_use(&s.person().spool).unwrap());

    // The person is still only flagged, as before.
    let mine = capture(&s.person(), "password: hunter2hunter2hunter2");
    let store = capture_store::projection::current(&s.person()).unwrap();
    let tgt = store.resolve(&mine).unwrap().unwrap();
    assert!(store.note(&tgt).unwrap().unwrap().credential);
}

#[test]
fn a_retired_agent_cannot_write_and_its_notes_remain() {
    let s = Scratch::new("retired");
    let agent = agents::add(&s.person(), "claude").unwrap();
    let page = pages::create(&s.agent("claude"), "Before", None)
        .unwrap()
        .name
        .unwrap();

    Session::open(s.person())
        .unwrap()
        .retire(&agent.pubkey, None)
        .unwrap();

    let refused = mutate::append(&s.agent("claude"), &page, "after");
    assert!(matches!(refused, Err(Error::Refused(_))), "{refused:?}");
    let store = capture_store::projection::current(&s.person()).unwrap();
    let tgt = store.resolve(&page).unwrap().unwrap();
    assert_eq!(
        store.note(&tgt).unwrap().unwrap().by.as_deref(),
        Some("claude")
    );
    let listed = agents::list(&s.person()).unwrap();
    assert_eq!(listed.len(), 1);
    assert!(listed[0].retired);
}

#[test]
fn an_unknown_agent_is_refused() {
    let s = Scratch::new("unknown");
    let refused = pages::create(&s.agent("nobody"), "Hello", None);
    assert!(
        matches!(refused, Err(Error::Refused(_))),
        "{:?}",
        refused.err()
    );
    let refused = pages::create(&s.agent("../identity"), "Hello", None);
    assert!(refused.is_err());
}

#[test]
fn an_agent_cannot_enrol_another_and_names_are_unique() {
    let s = Scratch::new("enrol");
    agents::add(&s.person(), "claude").unwrap();
    assert!(matches!(
        agents::add(&s.agent("claude"), "helper"),
        Err(Error::Refused(_))
    ));
    assert!(agents::add(&s.person(), "claude").is_err());
    assert!(agents::add(&s.person(), "Claude").is_err());
    let key = s.person().agents_dir().join("claude.key");
    let mode =
        std::os::unix::fs::PermissionsExt::mode(&std::fs::metadata(key).unwrap().permissions());
    assert_eq!(
        mode & 0o777,
        0o600,
        "a signing key is not readable by others"
    );
}

#[test]
fn an_agent_writing_does_not_move_the_devices_own_numbering() {
    // Each origin's watermark from its own events. Were the agent's sequence
    // numbers handed to the device's, the next open would think the log had
    // lost the device's events and give it a new origin.
    let s = Scratch::new("numbering");
    agents::add(&s.person(), "claude").unwrap();
    let me = Session::open(s.person()).unwrap().identity.dev();
    for i in 0..5 {
        pages::create(&s.agent("claude"), &format!("Page {i}"), None).unwrap();
    }
    let again = Session::open(s.person()).unwrap();
    assert_eq!(again.identity.dev(), me, "the device kept its origin");
    assert_eq!(again.startup, capture_store::startup::State::Normal);
}

#[test]
fn a_rebuild_still_knows_which_notes_are_an_agents() {
    let s = Scratch::new("rebuild");
    agents::add(&s.person(), "claude").unwrap();
    let mine = capture(&s.person(), "mine");
    pages::create(&s.agent("claude"), "Theirs", None).unwrap();
    Session::open(s.person())
        .unwrap()
        .rebuild_projection()
        .unwrap();
    let store = capture_store::projection::current(&s.person()).unwrap();
    assert_eq!(slugs(store.timeline(50).unwrap()), vec![mine]);
}

#[test]
fn an_agent_whose_log_went_backwards_does_not_reuse_a_position() {
    let s = Scratch::new("backwards");
    agents::add(&s.person(), "claude").unwrap();
    let events = s.person().events.clone();
    let saved = std::fs::read(&events).unwrap();
    pages::create(&s.agent("claude"), "One", None).unwrap();
    // The log as it was before the agent wrote: a restore from an old backup.
    for suffix in ["-wal", "-shm"] {
        let _ = std::fs::remove_file(format!("{}{suffix}", events.display()));
    }
    std::fs::write(&events, saved).unwrap();
    let refused = pages::create(&s.agent("claude"), "Two", None);
    assert!(
        matches!(refused, Err(Error::Refused(_))),
        "{:?}",
        refused.err()
    );
}

#[derive(Clone, Debug)]
enum Step {
    /// The person rewrites the page from what they last read.
    Edit(u8),
    /// The agent appends a line.
    Append(u8),
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 24, ..ProptestConfig::default() })]

    /// An append never loses what was there: whatever the order of the
    /// person's edits and the agent's appends, every appended line that the
    /// person did not later rewrite away is in the page, and each append's
    /// result holds the whole text it followed.
    #[test]
    fn appends_and_edits_never_lose_a_version(steps in prop::collection::vec(
        prop_oneof![any::<u8>().prop_map(Step::Edit), any::<u8>().prop_map(Step::Append)],
        1..8,
    )) {
        let s = Scratch::new("interleave");
        agents::add(&s.person(), "claude").unwrap();
        let page = pages::create(&s.person(), "Log", None).unwrap().name.unwrap();
        let text = |p: &StorePaths| {
            let st = capture_store::projection::current(p).unwrap();
            let tgt = st.resolve(&page).unwrap().unwrap();
            st.note(&tgt).unwrap().unwrap().text.unwrap()
        };
        for step in steps {
            let before = text(&s.person());
            match step {
                Step::Edit(n) => {
                    mutate::edit(&s.person(), &page, &format!("{before}\nperson {n}")).unwrap();
                }
                Step::Append(n) => {
                    mutate::append(&s.agent("claude"), &page, &format!("agent {n}")).unwrap();
                    let after = text(&s.person());
                    prop_assert!(after.starts_with(&before), "{before:?} -> {after:?}");
                    let line = format!("agent {n}");
                    prop_assert!(after.ends_with(&line), "{after:?}");
                }
            }
        }
    }
}

#[test]
fn what_a_store_does_on_opening_is_the_devices_whoever_opens_it() {
    // Opening reads captures nothing has read yet and writes what it makes of
    // them. Opened by the agent, that work must still be signed by the device:
    // the person's capture read as a task is the person's task.
    let s = Scratch::new("opening");
    agents::add(&s.person(), "claude").unwrap();
    let mut quick = Session::open_for_capture(s.person()).unwrap();
    let body = capture_store::session::capture_body("call the dentist tomorrow at 9").unwrap();
    let mine = quick.capture(&body).unwrap().slug;
    drop(quick);

    let bot = agent_dev(&s.person(), "claude");
    let before = capture_store::log::Log::open_read(&s.person().events)
        .unwrap()
        .count()
        .unwrap();
    drop(Session::open(s.agent("claude")).unwrap());
    let log = capture_store::log::Log::open_read(&s.person().events).unwrap();
    assert!(
        log.count().unwrap() > before,
        "a precondition: opening read the capture"
    );
    assert!(
        origins_of(&s.person(), &mine).iter().all(|d| *d != bot),
        "the agent signed the person's promotion"
    );
}
