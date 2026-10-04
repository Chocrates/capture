//! Nothing content-bearing survives a purge, and the next table that does
//! fails this rather than waiting to be found by hand.
//!
//! `purge_note` edits the projection in place instead of re-folding, so every
//! derived table has to be named in it. Four have now been found surviving one
//! — `mentions`, `list_items`, `ambient` and `redact_queue` — each by somebody
//! reading the function and noticing an omission.
//!
//! This asks the database instead. It enumerates every table carrying a `tgt`
//! column at runtime, so a table added next year is covered the day it is
//! added, and fails if any of them still holds a row for a destroyed note.
//!
//! Tables that legitimately keep a row are listed below with a reason. That
//! list is the point: each entry is a claim somebody made in writing, the same
//! shape as `tools/unwired-allow.txt`, and adding to it is a decision rather
//! than an omission.

use capture_store::StorePaths;
use capture_store::session::Session;

struct Scratch(std::path::PathBuf);

impl Scratch {
    fn new(tag: &str) -> Scratch {
        let p = std::env::temp_dir().join(format!(
            "capture-purged-{tag}-{}-{:?}",
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

/// Tables that may still hold a row for a destroyed note, and why.
///
/// Every entry is a claim. A table belongs here only if what it keeps is about
/// the note's *existence* rather than its *content* — §1.1 is about the
/// content, and an id or a count is not it.
const MAY_SURVIVE: &[(&str, &str)] = &[
    (
        "notes",
        "the row is the record that the note existed and was destroyed: its id, \
         its re-derived name and when the destruction happened. The text is set \
         to NULL by the purge itself.",
    ),
    (
        "note_versions",
        "event ids, predecessors and kinds — the version chain. Dropping it \
         would leave successors pointing at nothing. It holds no text.",
    ),
    (
        "parents",
        "one id pointing at another: where the note sat in the wiki. No text.",
    ),
    (
        "read_by",
        "which grammar last read the note, so the sweep does not read it for \
         ever. A version number.",
    ),
    (
        "review",
        "counts of how often it was offered and deferred. No text.",
    ),
    (
        "late_cancels",
        "the record that a cancellation arrived after the destruction — a fact \
         *about* the destruction, which has to outlive it to be reported.",
    ),
    (
        "divergences",
        "event ids of versions that diverged. No text.",
    ),
];

/// Every table with a `tgt` column, asked of the database rather than listed.
fn tables_keyed_by_tgt(db: &rusqlite::Connection) -> Vec<String> {
    let mut names: Vec<String> = {
        let mut st = db
            .prepare("SELECT name FROM sqlite_master WHERE type = \'table\'")
            .unwrap();
        let rows = st.query_map([], |r| r.get::<_, String>(0)).unwrap();
        rows.map(|r| r.unwrap()).collect()
    };
    names.retain(|t| {
        let mut st = match db.prepare(&format!("SELECT * FROM \"{t}\" LIMIT 0")) {
            Ok(s) => s,
            Err(_) => return false,
        };
        let has = st.column_names().contains(&"tgt");
        let _ = st.query([]);
        has
    });
    names.sort();
    names
}

#[test]
fn a_destroyed_note_leaves_nothing_but_the_fact_of_itself() {
    let sc = Scratch::new("nothing");
    let p = sc.paths();

    // A note that populates as many derived tables as one capture can: a tag,
    // a mention, a checklist, a promotion with a time, and a link from
    // somewhere else.
    let slug = {
        let mut s = Session::open_for_capture(p.clone()).unwrap();
        let body = capture_store::session::capture_body(
            "#vault lunch with @sarah about ^acme on 2026-11-03: milk, eggs, bread",
        )
        .unwrap();
        s.capture(&body).unwrap().slug
    };
    capture_store::promote::settle(&p).unwrap();

    let tgt = capture_store::projection::current(&p)
        .unwrap()
        .resolve(&slug)
        .unwrap()
        .expect("the note is named");

    capture_store::mutate::redact(&p, &slug).unwrap();
    {
        let mut s = Session::open(p.clone()).unwrap();
        assert_eq!(s.drain_due_at(PAST_THE_WINDOW).unwrap().destroyed, 1);
    }

    let store = capture_store::projection::current(&p).unwrap();
    let db = rusqlite::Connection::open_with_flags(
        store.path(),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();

    let mut survived = Vec::new();
    for table in tables_keyed_by_tgt(&db) {
        if MAY_SURVIVE.iter().any(|(t, _)| *t == table) {
            continue;
        }
        let n: i64 = db
            .query_row(
                &format!("SELECT COUNT(*) FROM \"{table}\" WHERE tgt = ?1"),
                rusqlite::params![tgt.0.to_vec()],
                |r| r.get(0),
            )
            .unwrap();
        if n > 0 {
            survived.push(format!("{table} ({n} row(s))"));
        }
    }

    assert!(
        survived.is_empty(),
        "a destroyed note still has rows in: {}.\n\
         Either the purge should clear them, or they belong in MAY_SURVIVE \
         with a reason saying what they keep is the note's existence rather \
         than its content.",
        survived.join(", ")
    );
}

#[test]
fn the_allow_list_names_only_tables_that_exist() {
    // A reason for a table that is gone is a claim nobody is checking, and it
    // would silently excuse a *new* table that happened to take the same name.
    let sc = Scratch::new("allowlist");
    let p = sc.paths();
    // Opened once first: `projection::current` reads the generation from
    // `ops.db`, which a freshly created store does not have until something
    // opens it for writing.
    drop(Session::open(p.clone()).unwrap());
    let store = capture_store::projection::current(&p).unwrap();
    let db = rusqlite::Connection::open_with_flags(
        store.path(),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    let real = tables_keyed_by_tgt(&db);

    for (name, _) in MAY_SURVIVE {
        assert!(
            real.iter().any(|t| t == name),
            "MAY_SURVIVE names {name}, which is not a table with a tgt column"
        );
    }
}
