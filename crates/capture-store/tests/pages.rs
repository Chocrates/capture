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
    assert_eq!(second.unnamed, Some(pages::Unnamed::Taken));
    assert_eq!(first.unnamed, None);
    assert_ne!(second.slug, first.slug);
    assert_eq!(kind(&paths, &second.slug).as_deref(), Some("page"));
}

#[test]
fn a_title_that_looks_like_a_credential_gives_the_page_no_name() {
    // The credential rule keeps such words out of slugs and exports (§4.3); an
    // alias made from the title would carry them into every [[link]] instead.
    let scratch = Scratch::new("credential-title");
    let paths = scratch.paths();
    let made = pages::create(&paths, "deploy token ghp_a1b2c3d4e5f6g7h8i9j0", None).unwrap();
    assert_eq!(made.name, None);
    assert_eq!(made.unnamed, Some(pages::Unnamed::Credential));
    let notice = made.notice().expect("the surface says why");
    assert!(notice.contains("credential"), "{notice}");
    assert!(
        !notice.contains("ghp_"),
        "the notice must not repeat it: {notice}"
    );
    let store = capture_store::projection::current(&paths).unwrap();
    assert!(
        store
            .resolve("deploy-token-ghp-a1b2c3d4e5f6g7h8i9j0")
            .unwrap()
            .is_none(),
        "no alias from a flagged title"
    );
    assert_eq!(kind(&paths, &made.slug).as_deref(), Some("page"));
}

fn flagged(paths: &StorePaths, name: &str) -> bool {
    let _ = Session::open(paths.clone()).unwrap();
    let store = capture_store::projection::current(paths).unwrap();
    let tgt = store.resolve(name).unwrap().unwrap();
    store.note(&tgt).unwrap().unwrap().credential
}

#[test]
fn a_credential_pasted_into_a_page_by_an_edit_flags_it() {
    // A page is written almost entirely by edits, and the check used to read
    // only the first capture (pages-that-hold-up D3).
    let scratch = Scratch::new("credential-edit");
    let paths = scratch.paths();
    let made = pages::create(&paths, "Server notes", None).unwrap();
    let name = made.name.unwrap();
    assert!(!flagged(&paths, &name));
    mutate::edit(
        &paths,
        &name,
        "Server notes\npassword: hunter2hunter2hunter2",
    )
    .unwrap();
    assert!(flagged(&paths, &name), "the edit's text is checked");

    // Taking the secret out again does not unflag: it is still in the history,
    // and in every export of it.
    mutate::edit(&paths, &name, "Server notes\nnothing here").unwrap();
    assert!(flagged(&paths, &name));

    // Saying it is safe clears it, and a later credential flags it again: the
    // person declared that text safe, not whatever is written next.
    mutate::unflag(&paths, &name).unwrap();
    assert!(!flagged(&paths, &name));
    mutate::edit(&paths, &name, "Server notes\nnothing here\napi key sk-abc").unwrap();
    assert!(flagged(&paths, &name));
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

/// Every edit event the log holds for a note, in the order written.
fn edits_of(paths: &StorePaths, name: &str) -> usize {
    let store = capture_store::projection::current(paths).unwrap();
    let tgt = store.resolve(name).unwrap().unwrap();
    capture_store::log::Log::open(&paths.events)
        .unwrap()
        .all()
        .unwrap()
        .iter()
        .filter(|e| e.header.kind == capture_core::Kind::EDIT && e.header.tgt == Some(tgt))
        .count()
}

fn head(paths: &StorePaths, name: &str) -> capture_core::EventId {
    let store = capture_store::projection::current(paths).unwrap();
    let tgt = store.resolve(name).unwrap().unwrap();
    store.head_of(&tgt).unwrap().unwrap_or(tgt)
}

#[test]
fn a_stale_edit_is_refused_and_writes_nothing() {
    let scratch = Scratch::new("expect");
    let paths = scratch.paths();
    let name = pages::create(&paths, "Packing", None)
        .unwrap()
        .name
        .unwrap();
    let read = head(&paths, &name);
    mutate::edit(&paths, &name, "Packing\n- passport").unwrap();
    let err = mutate::edit_expecting(&paths, &name, "Packing\n- socks", Some(read)).unwrap_err();
    assert_eq!(err.exit_code(), capture_store::ExitCode::Conflict, "{err}");
    let store = capture_store::projection::current(&paths).unwrap();
    let tgt = store.resolve(&name).unwrap().unwrap();
    assert_eq!(
        store.text_of(&tgt).unwrap().as_deref(),
        Some("Packing\n- passport")
    );
    assert_eq!(
        edits_of(&paths, &name),
        1,
        "the refused edit is not in the log"
    );

    // Read again, and it goes through.
    let fresh = head(&paths, &name);
    mutate::edit_expecting(&paths, &name, "Packing\n- passport\n- socks", Some(fresh)).unwrap();
    assert_eq!(edits_of(&paths, &name), 2);
}

#[test]
fn interleaved_writers_never_lose_a_version() {
    use proptest::prelude::*;
    // A script reads, the person may save in between, the script writes
    // naming what it read. Whatever the interleaving, a write that went through
    // is in the log, a refused one is not, and the note shows the last write
    // that went through (pages-that-hold-up G18).
    proptest!(ProptestConfig::with_cases(32), |(steps in prop::collection::vec(0u8..3, 2..7))| {
        let scratch = Scratch::new("interleave");
        let paths = scratch.paths();
        let name = pages::create(&paths, "Log", None).unwrap().name.unwrap();
        let mut read = head(&paths, &name);
        let mut stale = false;
        let mut written = 0usize;
        let mut last = String::from("Log");
        for (i, step) in steps.iter().enumerate() {
            let text = format!("Log\nline {i}");
            match step {
                0 => { read = head(&paths, &name); stale = false; }
                1 => {
                    mutate::edit(&paths, &name, &text).unwrap();
                    written += 1; last = text; stale = true;
                }
                _ => {
                    let r = mutate::edit_expecting(&paths, &name, &text, Some(read));
                    if stale {
                        prop_assert!(r.is_err(), "a stale expect went through");
                    } else {
                        prop_assert!(r.is_ok(), "{:?}", r);
                        written += 1; last = text; read = head(&paths, &name);
                    }
                }
            }
        }
        prop_assert_eq!(edits_of(&paths, &name), written);
        let store = capture_store::projection::current(&paths).unwrap();
        let tgt = store.resolve(&name).unwrap().unwrap();
        prop_assert_eq!(store.text_of(&tgt).unwrap(), Some(last));
        prop_assert_eq!(store.divergence_count(&tgt).unwrap(), 0);
    });
}

fn links(paths: &StorePaths, name: &str) -> Vec<capture_store::projection::LinkRow> {
    let _ = Session::open(paths.clone()).unwrap();
    let store = capture_store::projection::current(paths).unwrap();
    let tgt = store.resolve(name).unwrap().unwrap();
    store.links_from(&tgt).unwrap()
}

fn tgt(paths: &StorePaths, name: &str) -> capture_core::EventId {
    let store = capture_store::projection::current(paths).unwrap();
    store.resolve(name).unwrap().unwrap()
}

#[test]
fn renaming_a_page_breaks_its_links_and_never_retargets_them() {
    // G6 and G7: a link means what its name meant when it was written. A
    // rename shows it broken; a new page taking the freed name does not
    // inherit it.
    let scratch = Scratch::new("rename");
    let paths = scratch.paths();
    let first = pages::create(&paths, "Packing", None).unwrap();
    let first_tgt = tgt(&paths, "packing");
    let linker = capture(&paths, "remember the [[packing]] list");
    let l = &links(&paths, &linker)[0];
    assert_eq!(l.resolved, Some(first_tgt));
    assert!(!l.is_broken());

    mutate::set_alias(&paths, &first.slug, Some("packing-list")).unwrap();
    let l = &links(&paths, &linker)[0];
    assert_eq!(
        l.resolved,
        Some(first_tgt),
        "it still records what it meant"
    );
    assert_eq!(l.now, None);
    assert!(l.is_broken(), "a renamed target shows broken");

    let second = pages::create(&paths, "Packing", None).unwrap();
    assert_eq!(
        second.name.as_deref(),
        Some("packing"),
        "the freed name is free"
    );
    let l = &links(&paths, &linker)[0];
    assert_eq!(
        l.resolved,
        Some(first_tgt),
        "not retargeted to the new holder"
    );
    assert_eq!(l.now, Some(tgt(&paths, "packing")));
    assert!(l.is_broken());

    // A link written now means the new holder.
    let fresh = capture(&paths, "the [[packing]] page again");
    let l = &links(&paths, &fresh)[0];
    assert_eq!(l.resolved, Some(tgt(&paths, "packing")));
    assert!(!l.is_broken());
}

#[test]
fn a_link_written_before_its_page_reaches_it_once_it_exists() {
    // G8, changed 2026-11-15. It stayed unresolved "by design" until the
    // linking note was edited again, and building a wiki by hand ran straight
    // into it: links are written first and their pages after. A link that
    // found nothing when written has no meaning to protect, so it reaches
    // whatever takes its name. Links that did resolve keep their target (G6,
    // G7 above).
    let scratch = Scratch::new("forward");
    let paths = scratch.paths();
    let early = capture(&paths, "write up the [[future-plans]] soon");
    let l = &links(&paths, &early)[0];
    assert_eq!(l.resolved, None);
    assert!(!l.is_broken(), "never fulfilled is not broken");

    pages::create(&paths, "Future plans", None).unwrap();
    let page = tgt(&paths, "future-plans");
    let l = &links(&paths, &early)[0];
    assert_eq!(l.resolved, Some(page), "reaches the page made after it");
    assert!(!l.is_broken());
    let store = capture_store::projection::current(&paths).unwrap();
    assert_eq!(l.target_slug, store.slug_of(&page).unwrap());

    let back = store.links_to(&page).unwrap();
    assert_eq!(back.len(), 1, "and the page knows: {back:?}");
    assert_eq!(back[0].tgt, tgt(&paths, &early));
}

#[test]
fn a_link_can_name_a_page_by_its_title() {
    // `[[KV cache maths]]` for the page named `kv-cache-maths`: what people and
    // models write is the title they see.
    let scratch = Scratch::new("bytitle");
    let paths = scratch.paths();
    pages::create(&paths, "KV cache maths", None).unwrap();
    let page = tgt(&paths, "kv-cache-maths");

    // Written after the page exists: bound when written.
    let after = capture(&paths, "see [[KV cache maths]] and [[Qwen3.8-27B]]");
    let l = links(&paths, &after);
    assert_eq!(l[0].resolved, Some(page));
    assert_eq!(l[1].resolved, None);

    // Written before its page, by title: reaches it once it exists.
    pages::create(&paths, "Qwen3.8-27B", None).unwrap();
    let qwen = tgt(&paths, "qwen3-8-27b");
    assert_eq!(links(&paths, &after)[1].resolved, Some(qwen));

    let store = capture_store::projection::current(&paths).unwrap();
    assert_eq!(store.links_to(&qwen).unwrap().len(), 1);
    assert_eq!(store.links_to(&page).unwrap().len(), 1);
}

#[test]
fn a_page_linking_three_times_is_one_backlink() {
    let scratch = Scratch::new("dedupe");
    let paths = scratch.paths();
    pages::create(&paths, "Target", None).unwrap();
    let page = tgt(&paths, "target");
    let src = capture(&paths, "[[target]], again [[target]], and [[Target]]");
    let store = capture_store::projection::current(&paths).unwrap();
    let back = store.links_to(&page).unwrap();
    assert_eq!(back.len(), 1, "{back:?}");
    assert_eq!(back[0].tgt, tgt(&paths, &src));
}

#[test]
fn a_title_in_any_script_names_its_page_whole() {
    // G9. Each of the first three was split mid-word by the first version.
    for (title, want) in [
        ("हिन्दी नोट्स", Some("हिन्दी-नोट्स")),
        ("İstanbul trip", Some("istanbul-trip")),
        ("cafe\u{301} notes", Some("café-notes")),
        ("Café notes", Some("café-notes")),
        ("東京 旅行", Some("東京-旅行")),
        ("ملاحظات السفر", Some("ملاحظات-السفر")),
        ("Ελληνικά", Some("ελληνικά")),
        ("Car maintenance", Some("car-maintenance")),
        ("  Q3 -- plan!  ", Some("q3-plan")),
        ("🚗🔧", None),
        ("", None),
    ] {
        assert_eq!(pages::name_for(title).as_deref(), want, "{title:?}");
    }
}

#[test]
fn a_name_is_well_formed_and_stable_for_any_title() {
    use proptest::prelude::*;
    proptest!(ProptestConfig::with_cases(2048), |(title in "\\PC{0,40}")| {
        if let Some(n) = pages::name_for(&title) {
            prop_assert!(!n.is_empty());
            prop_assert!(!n.starts_with('-') && !n.ends_with('-'), "{:?}", n);
            prop_assert!(!n.contains("--"), "{:?}", n);
            prop_assert!(!n.chars().any(char::is_whitespace), "{:?}", n);
            prop_assert_eq!(n.to_lowercase(), n.clone());
            // A name, used as a title, names itself.
            prop_assert_eq!(pages::name_for(&n), Some(n.clone()));
        }
    });
}

/// Carry out every queued redaction now, as the drain would once the window
/// has passed.
fn destroy_due(paths: &StorePaths) {
    let mut s = Session::open(paths.clone()).unwrap();
    let ops = capture_store::ops::Ops::open(&paths.ops).unwrap();
    let queue = s.state.redact_queue().unwrap();
    for w in capture_store::due::due(&queue, u64::MAX / 2) {
        if let capture_store::due::Work::Destroy { redact, target } = w {
            capture_store::purge::execute(
                &mut s.log,
                &mut s.state,
                &ops,
                &paths.blobs,
                &redact,
                &target,
                u64::MAX / 2,
            )
            .unwrap();
        }
    }
}

#[test]
fn a_destroyed_page_leaves_its_links_broken_and_its_children_unfiled() {
    // G10, store half.
    let scratch = Scratch::new("redact-page");
    let paths = scratch.paths();
    let trip = pages::create(&paths, "Trip", None).unwrap();
    let child = pages::create(&paths, "Trip packing", Some("trip")).unwrap();
    let linker = capture(&paths, "see the [[trip]] page");
    assert!(!links(&paths, &linker)[0].is_broken());

    mutate::redact(&paths, &trip.slug).unwrap();
    destroy_due(&paths);

    let l = &links(&paths, &linker)[0];
    assert!(
        l.is_broken(),
        "a link to destroyed content shows broken: {l:?}"
    );
    let store = capture_store::projection::current(&paths).unwrap();
    let named = store.named().unwrap();
    let c = named
        .iter()
        .find(|n| n.alias == child.name.clone().unwrap())
        .expect("the child page is still a page");
    assert_eq!(c.parent, None, "not filed under a tombstone");
    assert!(
        named.iter().all(|n| n.alias != "trip"),
        "the destroyed page is not listed"
    );
}
