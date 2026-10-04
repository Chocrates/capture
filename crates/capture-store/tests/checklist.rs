//! Items of a capture: what survives an edit, and what does not.
//!
//! The defect this change exists for was silent and ordinary. Correcting a word
//! on a shopping list discarded what had already been bought — the `item_done`
//! event stayed in the log and simply stopped matching anything, so nothing
//! anywhere said it had happened.

// Reading a clock, which the purity lint otherwise refuses. Timing a read is
// not stamping an event, and the boundary that rule protects is the fold's.
#![allow(
    clippy::disallowed_methods,
    clippy::disallowed_types,
    reason = "measuring how long a read takes, not stamping an event"
)]

use capture_store::session::Session;
use capture_store::{StorePaths, mutate, promote};
use proptest::prelude::*;

struct Scratch(std::path::PathBuf);

impl Scratch {
    fn new(tag: &str) -> Scratch {
        let p = std::env::temp_dir().join(format!(
            "capture-checklist-{tag}-{}-{:?}",
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

/// The items of a capture, as `(text, done)`, in written order.
fn items(paths: &StorePaths, slug: &str) -> Vec<(String, bool)> {
    let store = settled(paths);
    let tgt = store.resolve(slug).unwrap().unwrap();
    store
        .items_of(&tgt)
        .unwrap()
        .into_iter()
        .map(|e| (e.text, e.done))
        .collect()
}

const NOW: u64 = 1_800_000_000_000;

#[test]
fn rewording_an_item_keeps_it_ticked() {
    // The defect, as an assertion. Before this, "2 of 3 left" became "3 of 3
    // left" because the tick was matched by the words that had just changed.
    let scratch = Scratch::new("reword");
    let paths = scratch.paths();
    let slug = capture(&paths, "#shop: milk, eggs, bread");

    promote::tick(&paths, "shop", "eggs", true, NOW).unwrap();
    assert_eq!(
        items(&paths, &slug),
        vec![
            ("milk".into(), false),
            ("eggs".into(), true),
            ("bread".into(), false),
        ]
    );

    mutate::edit(&paths, &slug, "#shop: milk, free-range eggs, bread").unwrap();
    assert_eq!(
        items(&paths, &slug),
        vec![
            ("milk".into(), false),
            ("free-range eggs".into(), true),
            ("bread".into(), false),
        ],
        "correcting a word discarded what had already been bought"
    );
}

#[test]
fn inserting_an_item_does_not_move_a_tick() {
    // The failure the position alone would have introduced: everything below
    // the insertion shifts, so every tick lands one row off.
    let scratch = Scratch::new("insert");
    let paths = scratch.paths();
    let slug = capture(&paths, "#shop: milk, eggs, bread");

    promote::tick(&paths, "shop", "bread", true, NOW).unwrap();
    mutate::edit(&paths, &slug, "#shop: butter, milk, eggs, bread").unwrap();

    assert_eq!(
        items(&paths, &slug),
        vec![
            ("butter".into(), false),
            ("milk".into(), false),
            ("eggs".into(), false),
            ("bread".into(), true),
        ],
        "an insertion moved a tick to the wrong row"
    );
}

#[test]
fn removing_an_item_does_not_hand_its_tick_to_another() {
    let scratch = Scratch::new("remove");
    let paths = scratch.paths();
    let slug = capture(&paths, "#shop: milk, eggs, bread");

    promote::tick(&paths, "shop", "eggs", true, NOW).unwrap();
    mutate::edit(&paths, &slug, "#shop: milk, bread").unwrap();

    assert_eq!(
        items(&paths, &slug),
        vec![("milk".into(), false), ("bread".into(), false)],
        "a removed item's tick was inherited by its neighbour"
    );

    // And putting it back brings the tick with it: the event was never
    // destroyed, only unmatched.
    mutate::edit(&paths, &slug, "#shop: milk, eggs, bread").unwrap();
    assert_eq!(items(&paths, &slug)[1], ("eggs".into(), true));
}

#[test]
fn two_items_reading_the_same_are_two_items() {
    // Untickable before this: `tick` matched by text, found two, and refused.
    let scratch = Scratch::new("dupes");
    let paths = scratch.paths();
    let slug = capture(&paths, "#shop: milk, milk, bread");

    // By words, it still cannot tell them apart, and says so rather than
    // guessing.
    assert!(matches!(
        promote::tick(&paths, "shop", "milk", true, NOW).unwrap(),
        capture_store::promote::Ticked::Ambiguous(2)
    ));

    // By position, there is nothing to be ambiguous about — which is what a
    // surface showing the list always had and could never say.
    promote::tick_at(&paths, &slug, 1, true).unwrap();
    assert_eq!(
        items(&paths, &slug),
        vec![
            ("milk".into(), false),
            ("milk".into(), true),
            ("bread".into(), false),
        ],
        "ticking one of two identical items ticked the wrong one"
    );
}

#[test]
fn a_tick_can_be_put_back() {
    let scratch = Scratch::new("untick");
    let paths = scratch.paths();
    let slug = capture(&paths, "#shop: milk, eggs");

    promote::tick_at(&paths, &slug, 0, true).unwrap();
    assert!(items(&paths, &slug)[0].1, "ticking did not take");

    promote::tick_at(&paths, &slug, 0, false).unwrap();
    assert!(!items(&paths, &slug)[0].1, "un-ticking did not take");
}

proptest! {
    // A store per case is real I/O, so the count is deliberately low.
    #![proptest_config(ProptestConfig::with_cases(16))]

    /// No tick ever lands on an item whose words nobody ticked.
    ///
    /// The one failure the design admits — rewording an item *and* inserting
    /// another above it in the same edit — is excluded by construction here:
    /// each case makes exactly one kind of edit.
    #[test]
    fn one_edit_never_moves_a_tick_to_a_word_nobody_ticked(
        n in 2usize..5,
        tick_at_idx in 0usize..4,
        edit in 0u8..3,
    ) {
        let scratch = Scratch::new("prop");
        let paths = scratch.paths();

        let words: Vec<String> = (0..n).map(|i| format!("item{i}")).collect();
        let slug = capture(&paths, &format!("#p: {}", words.join(", ")));

        let at = tick_at_idx % n;
        promote::tick_at(&paths, &slug, at as u32, true).unwrap();
        let ticked_word = words[at].clone();

        let after: Vec<String> = match edit {
            // Reword the ticked one.
            0 => {
                let mut w = words.clone();
                w[at] = format!("{}-corrected", w[at]);
                w
            }
            // Insert above everything.
            1 => {
                let mut w = vec![String::from("inserted")];
                w.extend(words.clone());
                w
            }
            // Remove the first, which is sometimes the ticked one.
            _ => words[1..].to_vec(),
        };
        mutate::edit(&paths, &slug, &format!("#p: {}", after.join(", "))).unwrap();

        let got = items(&paths, &slug);
        for (text, done) in &got {
            if !*done {
                continue;
            }
            // The only thing allowed to be ticked is the item somebody ticked,
            // by its words or by the words it was corrected to.
            prop_assert!(
                *text == ticked_word || text.starts_with(&ticked_word),
                "{text:?} came back ticked; only {ticked_word:?} was"
            );
        }
    }
}

#[test]
fn a_list_does_not_contain_the_capture_that_wrote_it() {
    // Reachable the moment a shopping list is promoted to a task, which is an
    // ordinary thing to do to one. The capture then appeared beneath its own
    // items and was counted with them: three items, four things outstanding.
    let scratch = Scratch::new("self");
    let paths = scratch.paths();
    let slug = capture(&paths, "#shop: milk, eggs, bread");

    promote::as_task(&paths, &slug, None).unwrap();

    let store = settled(&paths);
    let rows = store.list("shop", NOW).unwrap();
    assert_eq!(
        rows.len(),
        3,
        "the list contains itself: {:?}",
        rows.iter().map(|r| &r.text).collect::<Vec<_>>()
    );
    assert!(
        !rows.iter().any(|r| r.text.starts_with('#')),
        "the capture appeared as one of its own items"
    );

    // A task carrying the tag that is *not* the list capture still belongs
    // beside the items, which is what the union was for.
    let other = capture(&paths, "collect the parcel #shop");
    promote::as_task(&paths, &other, None).unwrap();
    assert_eq!(settled(&paths).list("shop", NOW).unwrap().len(), 4);
}

#[test]
fn an_untagged_checklist_belongs_to_its_capture_alone() {
    let scratch = Scratch::new("untagged");
    let paths = scratch.paths();
    let slug = capture(&paths, ": pack passport, charger, adapter");

    assert_eq!(
        items(&paths, &slug),
        vec![
            ("pack passport".into(), false),
            ("charger".into(), false),
            ("adapter".into(), false),
        ],
        "an untagged checklist did not expand"
    );

    // Tickable by position, like any other.
    promote::tick_at(&paths, &slug, 1, true).unwrap();
    assert!(
        items(&paths, &slug)[1].1,
        "ticking by position did not take"
    );

    // And gathered into no collection: writing no tag meant no filing, so an
    // empty name must not become a bucket every untagged list falls into.
    assert!(settled(&paths).list("", NOW).unwrap().is_empty());
}

#[test]
fn a_destroyed_capture_has_no_items() {
    let scratch = Scratch::new("redacted");
    let paths = scratch.paths();
    let slug = capture(&paths, "#shop: milk, eggs");
    assert_eq!(items(&paths, &slug).len(), 2);

    mutate::redact(&paths, &slug).unwrap();

    // Queued, not yet carried out. The items are still here, because
    // withholding the text is a read-time `CASE` and the expansion reads the
    // stored text. Asserted rather than glossed over: nothing leaks, but the
    // list is still drawable until the destruction runs.
    let store = settled(&paths);
    let tgt = store.resolve(&slug).unwrap().unwrap();
    assert_eq!(
        store.items_of(&tgt).unwrap().len(),
        2,
        "queued behaviour changed; update this comment"
    );

    // Destroyed. The text is gone, so there is nothing to expand, and
    // `purge_note` clears the rows that were already there.
    carry_out(&paths, 1_900_000_000_000);

    let store = settled(&paths);
    assert!(
        store.items_of(&tgt).unwrap().is_empty(),
        "a destroyed capture still listed its items"
    );
    assert!(store.list("shop", NOW).unwrap().is_empty());
}

/// Carry out every redaction that has come due.
fn carry_out(paths: &StorePaths, now: u64) {
    use capture_store::projection::{Store, generation_path};
    use capture_store::{due, log::Log, ops::Ops, purge};

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

#[test]
fn a_tick_that_matches_nothing_is_inert_and_comes_back() {
    // Not "held", and the distinction matters. `Held` is for an event this
    // build could not *understand* — a malformed body, an unknown kind — and is
    // re-examined after an upgrade. A tick that matches no item is understood
    // perfectly; it simply refers to something the capture does not say right
    // now.
    //
    // So it stays in the log and applies to nothing, and because reconciliation
    // runs on every expansion, putting the item back brings it straight back.
    // An edit does not destroy a record of something that happened.
    let scratch = Scratch::new("inert");
    let paths = scratch.paths();
    let slug = capture(&paths, "#shop: milk, eggs, bread");

    promote::tick_at(&paths, &slug, 1, true).unwrap();
    mutate::edit(&paths, &slug, "#shop: milk, bread").unwrap();
    assert!(
        items(&paths, &slug).iter().all(|(_, done)| !done),
        "a dropped item's tick was handed to something else"
    );

    mutate::edit(&paths, &slug, "#shop: milk, eggs, bread").unwrap();
    assert_eq!(
        items(&paths, &slug)[1],
        ("eggs".into(), true),
        "restoring the item did not restore the tick"
    );
}

#[test]
fn rewording_and_inserting_at_once_loses_the_tick_rather_than_moving_it() {
    // The case the design admits it cannot get right, asserted so that a future
    // fix changes this deliberately instead of discovering it.
    //
    // Reword an item *and* insert another above it in one edit: the position
    // shifted, so rule 1 misses; the words changed, so rule 2 misses; and the
    // count changed, so rule 3 refuses to compare positions at all.
    //
    // The outcome is a tick that matches **nothing**, which is the failure
    // worth choosing. Losing one is visible and recoverable — the event is
    // still in the log. Landing it on the wrong row is silent, and tells
    // somebody they already bought something they did not.
    let scratch = Scratch::new("both");
    let paths = scratch.paths();
    let slug = capture(&paths, "#shop: milk, eggs, bread");

    promote::tick_at(&paths, &slug, 1, true).unwrap();
    mutate::edit(&paths, &slug, "#shop: butter, milk, free-range eggs, bread").unwrap();

    let got = items(&paths, &slug);
    assert!(
        got.iter().all(|(_, done)| !done),
        "the tick landed on a row nobody ticked: {got:?}"
    );
}

/// What a checklist and a collection cost.
///
/// A measurement rather than an assertion, for the reason the others give: a
/// threshold in CI measures whatever else the machine was doing. The shape is
/// what matters — `items_of` is one indexed read, and `list` gathers across
/// captures and then reconciles every tick against every capture it found.
///
/// The entity index was 486ms until somebody looked, so this looks.
///
/// `cargo test -p capture-store --test checklist -- --ignored --nocapture`
#[test]
#[ignore = "a measurement, not an assertion"]
fn what_a_list_read_costs() {
    let scratch = Scratch::new("cost");
    let paths = scratch.paths();

    // Fifty captures under one tag, ten items each: a collection of five
    // hundred, which is a tag somebody has been writing to for a year.
    let mut slugs = Vec::new();
    for c in 0..50 {
        let items: Vec<String> = (0..10).map(|i| format!("thing {c}-{i}")).collect();
        slugs.push(capture(&paths, &format!("#big: {}", items.join(", "))));
    }
    // And ticks, because reconciliation is per tick per capture and is the part
    // most likely to have been written quadratically.
    for slug in slugs.iter().take(20) {
        for ord in 0..5 {
            promote::tick_at(&paths, slug, ord, true).unwrap();
        }
    }

    let store = settled(&paths);
    let tgt = store.resolve(&slugs[0]).unwrap().unwrap();

    let runs = 20;
    let began = std::time::Instant::now();
    for _ in 0..runs {
        assert_eq!(store.items_of(&tgt).unwrap().len(), 10);
    }
    let one = began.elapsed() / runs;

    let began = std::time::Instant::now();
    let gathered = store.list("big", NOW).unwrap();
    let all = began.elapsed();

    println!(
        "\n  50 captures, {} items, 100 ticks\n  one capture's checklist: {one:?} per read\n  the whole collection:    {all:?}\n",
        gathered.len(),
    );
}

#[test]
fn accepting_a_near_miss_makes_the_capture_a_list() {
    // The capture that started this, from a real archive: a grocery list for
    // the day, written as one, not recognised, and nothing saying why.
    let scratch = Scratch::new("nearmiss");
    let paths = scratch.paths();
    let slug = capture(
        &paths,
        "#groceries 7 apples, rye flour or bread, 1kg lentils, bag of carrots",
    );

    // Not a list, and the archive can say exactly why.
    assert!(items(&paths, &slug).is_empty());
    let said = capture_store::review::suggestion_for(
        "#groceries 7 apples, rye flour or bread, 1kg lentils, bag of carrots",
        None,
        NOW as i64,
    )
    .expect("a near miss");
    assert!(said.contains("#groceries:"), "it should say where: {said}");

    let fixed = mutate::accept_near_miss(&paths, &slug).unwrap();
    assert_eq!(
        fixed, "#groceries: 7 apples, rye flour or bread, 1kg lentils, bag of carrots",
        "the colon went in the wrong place"
    );

    assert_eq!(
        items(&paths, &slug)
            .into_iter()
            .map(|(t, _)| t)
            .collect::<Vec<_>>(),
        vec![
            "7 apples".to_string(),
            "rye flour or bread".into(),
            "1kg lentils".into(),
            "bag of carrots".into(),
        ],
        "accepting did not produce the list the text named"
    );

    // And there is nothing left to offer.
    assert!(
        capture_store::review::suggestion_for(&fixed, None, NOW as i64).is_none()
            || !capture_store::review::suggestion_for(&fixed, None, NOW as i64)
                .unwrap()
                .contains("looks like a list")
    );
}

#[test]
fn accepting_is_refused_where_there_is_nothing_to_fix() {
    let scratch = Scratch::new("nofix");
    let paths = scratch.paths();

    // Already a list.
    let listy = capture(&paths, "#shop: milk, eggs");
    assert!(mutate::accept_near_miss(&paths, &listy).is_err());

    // An ordinary tagged note, which is not a near miss and must not be edited.
    let note = capture(&paths, "#reading finished the long one");
    assert!(mutate::accept_near_miss(&paths, &note).is_err());

    // A colon with nothing after it needs items, not a character — so it says
    // that rather than writing an empty list.
    let bare = capture(&paths, "#groceries:");
    let err = mutate::accept_near_miss(&paths, &bare).unwrap_err();
    assert!(format!("{err}").contains("items"), "unhelpful: {err}");
}

#[test]
fn declining_leaves_the_capture_exactly_as_written() {
    let scratch = Scratch::new("decline");
    let paths = scratch.paths();
    // Three items, because two is below the bar the eagerness measurement set:
    // one comma fired on every ordinary tagged note it was tried against.
    let text = "#groceries 7 apples, 1kg lentils, bag of carrots";
    let slug = capture(&paths, text);

    // Reading the offer changes nothing, and it is still there afterwards.
    let store = settled(&paths);
    let tgt = store.resolve(&slug).unwrap().unwrap();
    assert_eq!(store.text_of(&tgt).unwrap().as_deref(), Some(text));
    assert!(capture_store::review::suggestion_for(text, None, NOW as i64).is_some());
    assert!(items(&paths, &slug).is_empty());
}
