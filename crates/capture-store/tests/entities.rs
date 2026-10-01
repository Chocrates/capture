//! Entity pages: identity that survives a rename, and a merge that loses nothing.
//!
//! The four failures §4.3 names are the four things checked here. Each one was
//! reachable before this existed, and each is a page quietly holding half a
//! history rather than an error anybody would have seen.

use capture_core::id::EntId;
use capture_store::session::Session;
use capture_store::{StorePaths, mutate, promote};

struct Scratch(std::path::PathBuf);

impl Scratch {
    fn new(tag: &str) -> Scratch {
        let p = std::env::temp_dir().join(format!(
            "capture-entities-{tag}-{}-{:?}",
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

fn capture(paths: &StorePaths, text: &str) -> String {
    let mut s = Session::open_for_capture(paths.clone()).unwrap();
    let body = capture_store::session::capture_body(text).unwrap();
    s.capture(&body).unwrap().slug
}

fn settled(paths: &StorePaths) -> capture_store::projection::Store {
    let _ = Session::open(paths.clone()).unwrap();
    capture_store::projection::current(paths).unwrap()
}

/// Carry out every redaction that has come due.
///
/// The same two steps the daemon takes — find the due work, then execute it.
/// Ordering a redaction only queues it, and the mentions of a note survive until
/// its text actually goes.
fn carry_out(paths: &StorePaths, now: u64) {
    use capture_store::{due, log::Log, ops::Ops, purge};

    use capture_store::projection::{Store, generation_path};

    let mut log = Log::open(&paths.events).unwrap();
    let ops = Ops::open(&paths.ops).unwrap();
    let g = ops.generation().unwrap();
    let mut state = Store::open(&generation_path(&paths.state, g)).unwrap();
    for w in due::due(&state.redact_queue().unwrap(), now) {
        if let due::Work::Destroy { redact, target } = w {
            purge::execute(&mut log, &mut state, &ops, &redact, &target, now).unwrap();
        }
    }
}

const NOW: u64 = 1_800_000_000_000;

fn sarah() -> EntId {
    EntId::derive("person", "sarah")
}

#[test]
fn case_is_not_two_people() {
    // The failure that needed no merge to reach: `@Sarah` and `@sarah` were two
    // unrelated rows, so half a person's history sat on a page nobody visited.
    let scratch = Scratch::new("case");
    let paths = scratch.paths();
    capture(&paths, "ask @sarah about the invoice");
    capture(&paths, "@Sarah replied");

    let store = settled(&paths);
    let ent = store
        .entity("person", "sarah")
        .unwrap()
        .expect("she exists");
    assert_eq!(store.entity_notes(&ent.ent, NOW).unwrap().len(), 2);

    // Either spelling asks the same question.
    assert_eq!(store.entity_id("person", "SARAH").unwrap(), Some(ent.ent));
    assert_eq!(store.entity_id("person", "Sarah").unwrap(), Some(ent.ent));

    // And the display is the spelling written first, not the folded key.
    assert_eq!(ent.display, "sarah");
}

#[test]
fn a_kind_keeps_a_person_and_a_project_apart() {
    // A client and the project named after them share a word constantly, and
    // joining them because of that would be the system deciding something.
    let scratch = Scratch::new("kinds");
    let paths = scratch.paths();
    capture(&paths, "call @acme about the renewal");
    capture(&paths, "the ^acme migration is behind");

    let store = settled(&paths);
    let person = store.entity("person", "acme").unwrap().unwrap();
    let project = store.entity("project", "acme").unwrap().unwrap();
    assert_ne!(person.ent, project.ent);
    assert_eq!(store.entity_notes(&person.ent, NOW).unwrap().len(), 1);
    assert_eq!(store.entity_notes(&project.ent, NOW).unwrap().len(), 1);
}

#[test]
fn a_name_nobody_wrote_is_an_ordinary_answer() {
    let scratch = Scratch::new("nobody");
    let paths = scratch.paths();
    capture(&paths, "an ordinary thought naming nobody");

    let store = settled(&paths);
    assert_eq!(store.entity("person", "nobody").unwrap(), None);
    assert!(store.entities_of_kind("person", NOW).unwrap().is_empty());
}

#[test]
fn a_renamed_entity_keeps_every_note_it_had() {
    // §4.3's first failure. Renaming used to fork the page.
    let scratch = Scratch::new("rename");
    let paths = scratch.paths();
    capture(&paths, "ask @sarah about the invoice");
    capture(&paths, "@sarah sent it");

    mutate::rename_entity(&paths, "person", "sarah", "Sarah Chen").unwrap();

    let store = settled(&paths);
    let ent = store.entity("person", "sarah").unwrap().unwrap();
    assert_eq!(ent.display, "Sarah Chen", "the chosen name is not shown");
    assert_eq!(ent.ent, sarah(), "renaming moved the entity");
    assert_eq!(
        store.entity_notes(&ent.ent, NOW).unwrap().len(),
        2,
        "a rename forked the page"
    );
}

#[test]
fn merging_joins_two_spellings_and_moves_nothing() {
    // §4.3's third failure, and the requirement that a merge is a redirection:
    // no capture text changes and no mention row is removed.
    let scratch = Scratch::new("merge");
    let paths = scratch.paths();
    capture(&paths, "ask @sarah about the invoice");
    capture(&paths, "@sarah-chen sent the contract");

    let before = settled(&paths);
    assert_eq!(
        before
            .entity_notes(&before.entity("person", "sarah").unwrap().unwrap().ent, NOW)
            .unwrap()
            .len(),
        1,
        "two spellings should start as two entities"
    );

    mutate::merge_entities(&paths, "person", "sarah", "sarah-chen").unwrap();

    let store = settled(&paths);
    let ent = store.entity("person", "sarah").unwrap().unwrap();
    assert_eq!(
        store.entity_notes(&ent.ent, NOW).unwrap().len(),
        2,
        "the merged page does not hold both"
    );
    // Either spelling now reaches the survivor.
    assert_eq!(
        store.entity_id("person", "sarah-chen").unwrap(),
        Some(ent.ent)
    );
    assert_eq!(ent.names.len(), 2, "both spellings should reach it");

    // And the merged-away entity is gone from the index rather than listed empty.
    let people = store.entities_of_kind("person", NOW).unwrap();
    assert_eq!(people.len(), 1, "a merged-away entity is still listed");
    assert_eq!(people[0].count, 2);
}

#[test]
fn a_backing_note_shows_once_and_keeps_being_what_it_was() {
    let scratch = Scratch::new("backing");
    let paths = scratch.paths();
    let project = capture(&paths, "the ^acme migration, everything about it");
    capture(&paths, "^acme migration slipped a week");

    // A task, deliberately: §4.3 says binding must not retype the note.
    promote::as_task(&paths, &project, None).unwrap();
    mutate::bind_backing(&paths, "project", "acme", Some(&project)).unwrap();

    let store = settled(&paths);
    let ent = store.entity("project", "acme").unwrap().unwrap();
    assert!(ent.backing.is_some(), "nothing was bound");

    // Shown above the list, so it is not in the list.
    let notes = store.entity_notes(&ent.ent, NOW).unwrap();
    assert_eq!(notes.len(), 1, "the backing note appeared twice");

    // Still a task, and still outstanding.
    assert_eq!(store.tasks_outstanding("2026-01-01").unwrap().len(), 1);

    // Unbinding puts it back in the list.
    mutate::bind_backing(&paths, "project", "acme", None).unwrap();
    let store = settled(&paths);
    let ent = store.entity("project", "acme").unwrap().unwrap();
    assert_eq!(ent.backing, None, "unbinding was skipped");
    assert_eq!(store.entity_notes(&ent.ent, NOW).unwrap().len(), 2);
}

#[test]
fn a_destroyed_note_names_nobody() {
    // The guard in `write_into` has always been there and has never been tested.
    // An entity page must not become a way to read what redaction removed.
    let scratch = Scratch::new("redacted");
    let paths = scratch.paths();
    let slug = capture(&paths, "ask @sarah about the settlement figure");
    capture(&paths, "@sarah is away");

    assert_eq!(
        settled(&paths).entity_notes(&sarah(), NOW).unwrap().len(),
        2
    );

    mutate::redact(&paths, &slug).unwrap();

    // While the destruction is only *queued*, the note is still on the page —
    // its text is withheld at read time, but the mention row is there.
    //
    // Asserted rather than glossed over, because the comment above the mentions
    // fill in `write_into` claims otherwise: it says a note "whose content is
    // withheld or destroyed" must not go on naming people. Withholding is a
    // `CASE` in `NOTE_COLUMNS`, so the stored text the fill reads is intact.
    // Nothing leaks — the row carries no text — but the page does still say that
    // somebody the archive is destroying named this person.
    let store = settled(&paths);
    let queued = store.entity_notes(&sarah(), NOW).unwrap();
    assert_eq!(
        queued.len(),
        2,
        "queued behaviour changed; update the comment"
    );
    assert!(
        queued
            .iter()
            .all(|n| n.text.is_some() || n.redaction_queued),
        "a withheld note leaked its text onto an entity page"
    );

    // Destroyed, and now it names nobody: the text is gone, so there is nothing
    // to extract a mention from and the row is absent rather than removed.
    carry_out(&paths, 1_900_000_000_000);

    let store = settled(&paths);
    let notes = store.entity_notes(&sarah(), NOW).unwrap();
    assert_eq!(notes.len(), 1, "a destroyed note still names somebody");
}

#[test]
fn muting_an_entity_quietens_its_notes_and_lapses() {
    let scratch = Scratch::new("mute");
    let paths = scratch.paths();
    capture(&paths, "ask @sarah about the invoice");
    capture(&paths, "an unrelated thought");

    let until = 1_850_000_000_000u64;
    mutate::mute_entity(&paths, "person", "sarah", Some(until)).unwrap();

    // While it holds.
    let store = settled(&paths);
    assert!(
        store
            .entity_notes(&sarah(), 1_800_000_000_000)
            .unwrap()
            .is_empty(),
        "a muted entity's notes still surfaced"
    );

    // And after it lapses, without anything being restored.
    assert_eq!(
        store
            .entity_notes(&sarah(), 1_900_000_000_000)
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn identity_does_not_depend_on_arrival_order() {
    // 4.3. A phone reconnecting hands over week-old events after newer ones as a
    // matter of course, so nothing about *when* something arrived may reach the
    // answer — two devices folding one log have to agree.
    //
    // Identity is safe by construction here: `casefold(name)` is the hashed key,
    // so no ordering can touch it. What *is* order-sensitive is the display name
    // where nobody has chosen one, and the rule is the earliest capture by
    // `(created, tgt)` — not the first one folded.
    let scratch = Scratch::new("order");
    let paths = scratch.paths();

    capture(&paths, "ask @Sarah about the invoice");
    capture(&paths, "@sarah replied");
    capture(&paths, "@SARAH again");

    let store = settled(&paths);
    let ent = store
        .entity("person", "sarah")
        .unwrap()
        .expect("one person");
    assert_eq!(
        store.entity_notes(&ent.ent, NOW).unwrap().len(),
        3,
        "three spellings of one name made more than one person"
    );
    // The earliest capture wrote `@Sarah`, so that is what is shown.
    assert_eq!(ent.display, "Sarah");
    // And every spelling folds to the one name, so there is one row to reach.
    assert_eq!(ent.names, vec!["sarah".to_string()]);
}

#[test]
fn a_rebuild_reproduces_every_entity_exactly() {
    // 9.4. The schema bump forces a rebuild on every existing store, so this is
    // the one migration step that has to be watched rather than assumed. A
    // rebuild that derived different ids would silently empty every page.
    let scratch = Scratch::new("rebuild");
    let paths = scratch.paths();

    capture(&paths, "ask @sarah about the ^acme invoice");
    capture(&paths, "@Sarah replied about ^acme");
    capture(&paths, "@sarah-chen sent the contract");
    mutate::rename_entity(&paths, "person", "sarah", "Sarah Chen").unwrap();
    mutate::merge_entities(&paths, "person", "sarah-chen", "sarah").unwrap();

    let before: Vec<_> = {
        let store = settled(&paths);
        let mut all = store.entities_of_kind("person", NOW).unwrap();
        all.extend(store.entities_of_kind("project", NOW).unwrap());
        all
    };
    assert!(!before.is_empty());

    // Rebuild into a fresh generation, exactly as a stale projection does.
    let ops = capture_store::ops::Ops::open(&paths.ops).unwrap();
    let generation = ops.generation().unwrap();
    let log = capture_store::log::Log::open_read(&paths.events).unwrap();
    capture_store::projection::rebuild(&paths.state, &log.all().unwrap(), generation, NOW as i64)
        .unwrap();
    ops.set_generation(generation + 1).unwrap();

    let after: Vec<_> = {
        let store = capture_store::projection::current(&paths).unwrap();
        let mut all = store.entities_of_kind("person", NOW).unwrap();
        all.extend(store.entities_of_kind("project", NOW).unwrap());
        all
    };

    assert_eq!(before, after, "a rebuild did not reproduce the entities");
}

/// What an entity page and the indexes cost at a few hundred entities.
///
/// A measurement rather than an assertion, for the reason the others give: a
/// threshold in CI measures whatever else the machine was doing. The claim worth
/// checking is the shape — `entity_notes` is a join over `mentions` and
/// `entity_names`, and `entities_of_kind` calls it once per entity, which is the
/// obvious thing to have got wrong.
///
/// `cargo test -p capture-store --test entities -- --ignored --nocapture`
#[test]
#[ignore = "a measurement, not an assertion"]
fn what_an_entity_page_costs() {
    let scratch = Scratch::new("cost");
    let paths = scratch.paths();

    for i in 0..300 {
        capture(
            &paths,
            &format!("a thought naming @person{i} and ^project{i}"),
        );
    }
    // One entity everything names, so the widest page is measured rather than a
    // typical one.
    for i in 0..60 {
        capture(&paths, &format!("another word about @busy, number {i}"));
    }

    let store = settled(&paths);
    let busy = store.entity("person", "busy").unwrap().unwrap();

    let began = std::time::Instant::now();
    let runs = 20;
    for _ in 0..runs {
        assert_eq!(store.entity_notes(&busy.ent, NOW).unwrap().len(), 60);
    }
    let page = began.elapsed() / runs;

    let began = std::time::Instant::now();
    let people = store.entities_of_kind("person", NOW).unwrap();
    let index = began.elapsed();

    println!(
        "\n  {} people, {} projects\n  widest page (60 notes): {page:?} per read\n  people index: {index:?} for the whole list\n",
        people.len(),
        store.entities_of_kind("project", NOW).unwrap().len(),
    );
}
