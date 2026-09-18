//! A list, end to end, against a real store.
//!
//! The point of this file is that it runs before any surface exists. §3.6 says
//! lists are tags with a display order, and everything underneath that — the
//! expansion, the ticking, and the matching that survives a rebuild — is either
//! true here or it is not true anywhere. A list screen written on top of a
//! broken expansion looks fine right up until somebody's shopping disappears.

// The clock, because ticking writes a capture stamped with it and the mute
// horizon a list is read against is a real timestamp.
#![allow(
    clippy::disallowed_methods,
    clippy::disallowed_types,
    reason = "reading a list takes the same now a surface would pass"
)]

use capture_store::promote::Ticked;
use capture_store::session::Session;
use capture_store::{StorePaths, promote};

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

struct Scratch(std::path::PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let p = std::env::temp_dir().join(format!("capture-lists-{name}-{}", std::process::id()));
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

/// Settle the projection, the way opening any session does.
fn settle(paths: &StorePaths) {
    let _ = Session::open(paths.clone()).unwrap();
}

fn read(paths: &StorePaths, tag: &str) -> Vec<capture_store::projection::ListEntry> {
    let store =
        capture_store::projection::Store::open_read(&capture_store::projection::generation_path(
            &paths.state,
            capture_store::ops::Ops::open_read(&paths.ops)
                .unwrap()
                .generation()
                .unwrap(),
        ))
        .unwrap();
    store.list(tag, now()).unwrap()
}

#[test]
fn a_list_capture_expands_into_items() {
    let scratch = Scratch::new("expand");
    let paths = scratch.paths();

    capture(&paths, "#groceries: milk, eggs, bread");
    settle(&paths);

    let list = read(&paths, "groceries");
    let texts: Vec<_> = list.iter().map(|e| e.text.as_str()).collect();
    assert_eq!(
        texts,
        ["milk", "eggs", "bread"],
        "written order is display order"
    );
    assert!(
        list.iter().all(|e| !e.done),
        "nothing is done before it is ticked"
    );
    assert!(
        list.iter().all(|e| e.task.is_none()),
        "an unticked item has nothing in the log about it",
    );
}

#[test]
fn one_gesture_is_one_event() {
    let scratch = Scratch::new("one-event");
    let paths = scratch.paths();

    capture(&paths, "#groceries: milk, eggs, bread");
    settle(&paths);

    let store =
        capture_store::projection::Store::open_read(&capture_store::projection::generation_path(
            &paths.state,
            capture_store::ops::Ops::open_read(&paths.ops)
                .unwrap()
                .generation()
                .unwrap(),
        ))
        .unwrap();
    assert_eq!(
        store.note_count().unwrap(),
        1,
        "three items, one note — items are derived, not captured",
    );
}

#[test]
fn ticking_an_item_records_what_happened() {
    let scratch = Scratch::new("tick");
    let paths = scratch.paths();

    capture(&paths, "#groceries: milk, eggs, bread");
    settle(&paths);

    assert_eq!(
        promote::tick(&paths, "groceries", "milk", true, now()).unwrap(),
        Ticked::Done
    );
    settle(&paths);

    let list = read(&paths, "groceries");
    let milk = list
        .iter()
        .find(|e| e.text == "milk")
        .expect("milk is on the list");
    assert!(milk.done, "milk is bought");
    assert!(
        milk.task.is_none(),
        "an item is never a task — completion is an event about the list capture",
    );

    let eggs = list
        .iter()
        .find(|e| e.text == "eggs")
        .expect("eggs are on the list");
    assert!(!eggs.done, "ticking one item ticks only that one");

    assert_eq!(
        list.iter().filter(|e| e.text == "milk").count(),
        1,
        "a ticked item appears once, not as both an item and its task",
    );
}

#[test]
fn the_capture_is_not_rewritten_by_ticking() {
    let scratch = Scratch::new("not-rewritten");
    let paths = scratch.paths();

    let slug = capture(&paths, "#groceries: milk, eggs");
    settle(&paths);
    promote::tick(&paths, "groceries", "milk", true, now()).unwrap();
    settle(&paths);

    let store =
        capture_store::projection::Store::open_read(&capture_store::projection::generation_path(
            &paths.state,
            capture_store::ops::Ops::open_read(&paths.ops)
                .unwrap()
                .generation()
                .unwrap(),
        ))
        .unwrap();
    let tgt = store.resolve(&slug).unwrap().expect("the list capture");
    let note = store.note(&tgt).unwrap().expect("its row");
    assert_eq!(
        note.text.as_deref(),
        Some("#groceries: milk, eggs"),
        "promotion annotates and never rewrites the capture (§3.1.3)",
    );
}

/// The property the whole identity scheme exists for.
#[test]
fn a_tick_survives_a_rebuild() {
    let scratch = Scratch::new("rebuild");
    let paths = scratch.paths();

    capture(&paths, "#groceries: milk, eggs, bread");
    settle(&paths);
    promote::tick(&paths, "groceries", "milk", true, now()).unwrap();
    settle(&paths);

    // A full rebuild from the log, which is what the schema bump triggers on a
    // real archive and the only honest way to assert derived data survives.
    Session::open(paths.clone())
        .unwrap()
        .rebuild_projection()
        .unwrap();

    let list = read(&paths, "groceries");
    let texts: Vec<_> = list.iter().map(|e| e.text.as_str()).collect();
    assert_eq!(
        texts,
        ["milk", "eggs", "bread"],
        "the expansion is reproduced exactly"
    );
    assert!(
        list.iter().find(|e| e.text == "milk").unwrap().done,
        "and the tick is still there — completion is in the log, not in the projection",
    );
}

#[test]
fn ticking_twice_is_ticking_once() {
    let scratch = Scratch::new("idempotent");
    let paths = scratch.paths();

    capture(&paths, "#groceries: milk");
    settle(&paths);

    assert_eq!(
        promote::tick(&paths, "groceries", "milk", true, now()).unwrap(),
        Ticked::Done
    );
    settle(&paths);
    assert_eq!(
        promote::tick(&paths, "groceries", "milk", true, now()).unwrap(),
        Ticked::Already,
        "the second tick says so rather than claiming to have acted",
    );
    settle(&paths);

    let list = read(&paths, "groceries");
    assert_eq!(
        list.iter().filter(|e| e.text == "milk").count(),
        1,
        "one item, one task"
    );
}

#[test]
fn unticking_is_the_same_path_backwards() {
    let scratch = Scratch::new("untick");
    let paths = scratch.paths();

    capture(&paths, "#groceries: milk");
    settle(&paths);
    promote::tick(&paths, "groceries", "milk", true, now()).unwrap();
    settle(&paths);
    assert_eq!(
        promote::tick(&paths, "groceries", "milk", false, now()).unwrap(),
        Ticked::Done
    );
    settle(&paths);

    let list = read(&paths, "groceries");
    assert_eq!(list.len(), 1, "no second task was made by reopening");
    assert!(!list[0].done, "open again");
}

#[test]
fn two_items_reading_the_same_are_not_guessed_at() {
    let scratch = Scratch::new("ambiguous");
    let paths = scratch.paths();

    capture(&paths, "#groceries: milk, milk");
    settle(&paths);

    assert_eq!(
        promote::tick(&paths, "groceries", "milk", true, now()).unwrap(),
        Ticked::Ambiguous(2),
        "completion matches by text, so identical items are indistinguishable",
    );

    let list = read(&paths, "groceries");
    assert!(list.iter().all(|e| !e.done), "and neither was changed");
}

#[test]
fn an_item_that_is_not_on_the_list_is_not_invented() {
    let scratch = Scratch::new("absent");
    let paths = scratch.paths();

    capture(&paths, "#groceries: milk");
    settle(&paths);

    assert_eq!(
        promote::tick(&paths, "groceries", "caviar", true, now()).unwrap(),
        Ticked::NoSuchItem,
    );
    assert_eq!(read(&paths, "groceries").len(), 1, "nothing was added");
}

#[test]
fn two_captures_under_one_tag_read_as_one_list() {
    let scratch = Scratch::new("two-captures");
    let paths = scratch.paths();

    capture(&paths, "#groceries: milk, eggs");
    capture(&paths, "#groceries: bread");
    settle(&paths);

    let texts: Vec<_> = read(&paths, "groceries")
        .iter()
        .map(|e| e.text.clone())
        .collect();
    assert_eq!(texts.len(), 3, "one tag, one list: {texts:?}");
    for want in ["milk", "eggs", "bread"] {
        assert!(texts.iter().any(|t| t == want), "{want} is on the list");
    }
}

/// §3.7: a collection is a saved query, so a task tagged by hand joins the list
/// it belongs to without being copied into it.
#[test]
fn a_task_tagged_by_hand_joins_the_list() {
    let scratch = Scratch::new("hand-tagged");
    let paths = scratch.paths();

    capture(&paths, "#groceries: milk");
    let loose = capture(&paths, "buy batteries");
    settle(&paths);
    capture_store::mutate::tag(&paths, &loose, "groceries").unwrap();
    promote::as_task(&paths, &loose, None).unwrap();
    settle(&paths);

    let list = read(&paths, "groceries");
    let texts: Vec<_> = list.iter().map(|e| e.text.as_str()).collect();
    assert!(texts.contains(&"milk"), "the written item");
    assert!(
        texts.contains(&"buy batteries"),
        "and the one tagged into place"
    );

    let item = list.iter().find(|e| e.text == "milk").unwrap();
    assert_eq!(item.ord, Some(0), "a written item keeps its place");
    let tagged = list.iter().find(|e| e.text == "buy batteries").unwrap();
    assert_eq!(
        tagged.ord, None,
        "a loose task has no place in anybody's order"
    );
}

/// Editing a list is where a position-based identity would have lost the tick.
#[test]
fn a_tick_survives_an_edit_that_moves_the_item() {
    let scratch = Scratch::new("edit");
    let paths = scratch.paths();

    let slug = capture(&paths, "#groceries: milk, eggs");
    settle(&paths);
    promote::tick(&paths, "groceries", "eggs", true, now()).unwrap();
    settle(&paths);

    // A new item in front of it: `eggs` moves from position 1 to position 2.
    capture_store::mutate::edit(&paths, &slug, "#groceries: bread, milk, eggs").unwrap();
    settle(&paths);

    let list = read(&paths, "groceries");
    let eggs = list
        .iter()
        .find(|e| e.text == "eggs")
        .expect("eggs are still on the list");
    assert!(
        eggs.done,
        "the tick followed the item, not its position — which is the whole reason \
         completion is a task in the log rather than a flag on a row",
    );
    assert!(
        !list.iter().find(|e| e.text == "bread").unwrap().done,
        "and did not land on whatever took its old place",
    );
}

/// The reason completion stopped minting notes.
#[test]
fn ticking_mints_nothing() {
    let scratch = Scratch::new("mints-nothing");
    let paths = scratch.paths();

    capture(&paths, "#groceries: milk, eggs, bread");
    settle(&paths);

    for item in ["milk", "eggs", "bread"] {
        promote::tick(&paths, "groceries", item, true, now()).unwrap();
    }
    settle(&paths);

    let store =
        capture_store::projection::Store::open_read(&capture_store::projection::generation_path(
            &paths.state,
            capture_store::ops::Ops::open_read(&paths.ops)
                .unwrap()
                .generation()
                .unwrap(),
        ))
        .unwrap();

    assert_eq!(
        store.note_count().unwrap(),
        1,
        "the whole shopping done, and the timeline still holds one capture — \
         the first version of this left a note called `milk` that nobody wrote",
    );

    let list = read(&paths, "groceries");
    assert_eq!(list.len(), 3, "and the list still has its three items");
    assert!(list.iter().all(|e| e.done), "all of them done");
}

/// A tick outlives an edit that drops its item, and comes back with it.
#[test]
fn an_edit_that_drops_an_item_does_not_destroy_its_tick() {
    let scratch = Scratch::new("drop-restore");
    let paths = scratch.paths();

    let slug = capture(&paths, "#groceries: milk, eggs");
    settle(&paths);
    promote::tick(&paths, "groceries", "eggs", true, now()).unwrap();
    settle(&paths);

    capture_store::mutate::edit(&paths, &slug, "#groceries: milk").unwrap();
    settle(&paths);
    let list = read(&paths, "groceries");
    assert_eq!(list.len(), 1, "eggs are off the list");

    // Put it back. The completion was never destroyed — an edit is not a
    // reason to forget something that actually happened.
    capture_store::mutate::edit(&paths, &slug, "#groceries: milk, eggs").unwrap();
    settle(&paths);

    let list = read(&paths, "groceries");
    let eggs = list
        .iter()
        .find(|e| e.text == "eggs")
        .expect("eggs are back");
    assert!(eggs.done, "and so is the tick they had");
}
