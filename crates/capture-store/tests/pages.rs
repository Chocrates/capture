//! Pages: notes written to be read and revised, never read for a type.
//!
//! Asked for on 2026-10-13. A page starts from a title or from a capture, is
//! never promoted to a task, event or series however its text changes, can be
//! turned back into a capture, and has checklist lines ticked by line.

#![allow(
    clippy::disallowed_methods,
    clippy::disallowed_types,
    reason = "the occurrence horizon is measured from the real now"
)]

use capture_store::session::Session;
use capture_store::{StorePaths, mutate, pages, promote};

struct Scratch(std::path::PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let p = std::env::temp_dir().join(format!("capture-pages-{name}-{}", std::process::id()));
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

/// What the note is now, after anything outstanding has been read.
fn kind(paths: &StorePaths, name: &str) -> Option<String> {
    let _ = Session::open(paths.clone()).unwrap();
    let store = capture_store::projection::current(paths).unwrap();
    let tgt = store.resolve(name).unwrap().unwrap();
    store.promotion_of(&tgt).unwrap().map(|p| p.kind)
}

fn occurrences(paths: &StorePaths, name: &str) -> usize {
    let store = capture_store::projection::current(paths).unwrap();
    let tgt = store.resolve(name).unwrap().unwrap();
    store.occurrences_of(&tgt).unwrap().len()
}

fn items(paths: &StorePaths, name: &str) -> Vec<(String, bool)> {
    let _ = Session::open(paths.clone()).unwrap();
    let store = capture_store::projection::current(paths).unwrap();
    let tgt = store.resolve(name).unwrap().unwrap();
    store
        .items_of(&tgt)
        .unwrap()
        .into_iter()
        .map(|e| (e.text, e.done))
        .collect()
}

#[test]
fn a_page_is_never_read_for_a_type_however_it_is_edited() {
    let scratch = Scratch::new("never");
    let paths = scratch.paths();
    let made = pages::create(&paths, "Gym routine", None).unwrap();
    assert_eq!(kind(&paths, &made.slug).as_deref(), Some("page"));

    mutate::edit(
        &paths,
        &made.slug,
        "Gym routine\n\nevery monday at 2pm: legs\nthursday 10am: arms",
    )
    .unwrap();
    assert_eq!(
        kind(&paths, &made.slug).as_deref(),
        Some("page"),
        "an edit re-read it"
    );
    assert_eq!(
        occurrences(&paths, &made.slug),
        0,
        "the calendar got a series"
    );

    // And back: a capture again, read as one.
    promote::reread(&paths, &made.slug).unwrap();
    assert_eq!(kind(&paths, &made.slug).as_deref(), Some("series"));
}

#[test]
fn a_new_page_is_named_by_its_title_where_the_name_is_free() {
    let scratch = Scratch::new("named");
    let paths = scratch.paths();
    let first = pages::create(&paths, "Car maintenance", None).unwrap();
    assert_eq!(first.name.as_deref(), Some("car-maintenance"));
    let store = capture_store::projection::current(&paths).unwrap();
    let tgt = store
        .resolve("car-maintenance")
        .unwrap()
        .expect("the name resolves");
    assert_eq!(store.note(&tgt).unwrap().unwrap().slug, first.slug);
    assert_eq!(
        store.text_of(&tgt).unwrap().as_deref(),
        Some("Car maintenance")
    );

    let second = pages::create(&paths, "Car maintenance", None).unwrap();
    assert_eq!(second.name, None, "a taken name is not reused");
    assert_ne!(second.slug, first.slug);
    assert_eq!(kind(&paths, &second.slug).as_deref(), Some("page"));
}

#[test]
fn a_page_can_start_under_another() {
    let scratch = Scratch::new("under");
    let paths = scratch.paths();
    let car = pages::create(&paths, "Car", None).unwrap();
    let tyres = pages::create(&paths, "Tyres", Some("car")).unwrap();
    let store = capture_store::projection::current(&paths).unwrap();
    let named = store.named().unwrap();
    let tyres = named.iter().find(|n| n.slug == tyres.slug).expect("named");
    assert_eq!(
        tyres.parent.as_deref(),
        Some(car.slug.as_str()),
        "tyres is not under car"
    );
}

#[test]
fn a_capture_becomes_a_page_and_keeps_its_words() {
    let scratch = Scratch::new("make");
    let paths = scratch.paths();
    let slug = capture(&paths, "dentist thursday 2pm");
    assert_eq!(kind(&paths, &slug).as_deref(), Some("event"));

    pages::make(&paths, &slug).unwrap();
    assert_eq!(kind(&paths, &slug).as_deref(), Some("page"));
    let store = capture_store::projection::current(&paths).unwrap();
    let tgt = store.resolve(&slug).unwrap().unwrap();
    assert_eq!(
        store.text_of(&tgt).unwrap().as_deref(),
        Some("dentist thursday 2pm")
    );
    assert!(
        store.promotion_of(&tgt).unwrap().unwrap().pinned,
        "a page is a person's word"
    );
}

#[test]
fn checklist_lines_are_ticked_by_line_without_an_edit() {
    let scratch = Scratch::new("ticks");
    let paths = scratch.paths();
    let page = pages::create(&paths, "Packing", None).unwrap();
    let text = "Packing\n\n- passport\n- charger\n[x] tickets\nsome prose";
    mutate::edit(&paths, &page.slug, text).unwrap();
    assert_eq!(
        items(&paths, &page.slug),
        vec![
            ("passport".into(), false),
            ("charger".into(), false),
            ("tickets".into(), true),
        ]
    );

    promote::tick_at(&paths, &page.slug, 1, true).unwrap();
    assert_eq!(items(&paths, &page.slug)[1], ("charger".into(), true));
    let store = capture_store::projection::current(&paths).unwrap();
    let tgt = store.resolve(&page.slug).unwrap().unwrap();
    assert_eq!(
        store.text_of(&tgt).unwrap().as_deref(),
        Some(text),
        "a tick rewrote the text"
    );

    // Reordered, and the tick follows its item.
    mutate::edit(
        &paths,
        &page.slug,
        "Packing\n\n- charger\n- passport\n[x] tickets\n- adapter",
    )
    .unwrap();
    assert_eq!(
        items(&paths, &page.slug),
        vec![
            ("charger".into(), true),
            ("passport".into(), false),
            ("tickets".into(), true),
            ("adapter".into(), false),
        ]
    );

    // A line written ticked can be unticked on the page.
    promote::tick_at(&paths, &page.slug, 2, false).unwrap();
    assert_eq!(items(&paths, &page.slug)[2], ("tickets".into(), false));
}

#[test]
fn a_capture_is_not_read_for_checklist_lines() {
    let scratch = Scratch::new("capture-lines");
    let paths = scratch.paths();
    let slug = capture(&paths, "trip\n- passport\n- charger");
    assert!(
        items(&paths, &slug).is_empty(),
        "only a page has checklist lines"
    );
}

#[test]
fn completions_offer_pages_people_projects_and_tags_by_prefix() {
    let scratch = Scratch::new("complete");
    let paths = scratch.paths();
    pages::create(&paths, "Car maintenance", None).unwrap();
    capture(&paths, "call @sarah about ^acme-migration #cars");
    let _ = Session::open(paths.clone()).unwrap();
    let store = capture_store::projection::current(&paths).unwrap();

    let names = |sigil: &str, prefix: &str| -> Vec<String> {
        pages::completions(&store, sigil, prefix, 10, u64::MAX / 2)
            .unwrap()
            .into_iter()
            .map(|c| c.insert)
            .collect()
    };
    assert!(names("[[", "car").contains(&"car-maintenance".to_string()));
    assert_eq!(names("@", "sa"), vec!["sarah".to_string()]);
    assert_eq!(names("^", "acme"), vec!["acme-migration".to_string()]);
    assert_eq!(names("#", "ca"), vec!["cars".to_string()]);
    assert!(names("@", "zz").is_empty());
}

#[test]
fn a_page_can_start_as_a_persons_page() {
    let scratch = Scratch::new("of");
    let paths = scratch.paths();
    capture(&paths, "coffee with @sarah");
    let made = pages::create_for(&paths, "person", "sarah", "Sarah").unwrap();
    assert_eq!(kind(&paths, &made.slug).as_deref(), Some("page"));

    let store = capture_store::projection::current(&paths).unwrap();
    let ent = store.entity("person", "sarah").unwrap().unwrap();
    let tgt = store.resolve(&made.slug).unwrap().unwrap();
    assert_eq!(ent.backing, Some(tgt), "the page is not sarah's");

    // Somebody nobody has named yet is refused, and nothing is left behind.
    let before = store.timeline(100).unwrap().len();
    assert!(pages::create_for(&paths, "person", "nobody", "Nobody").is_err());
    let after = capture_store::projection::current(&paths)
        .unwrap()
        .timeline(100)
        .unwrap()
        .len();
    assert_eq!(after, before, "a refused page left a capture behind");
}
