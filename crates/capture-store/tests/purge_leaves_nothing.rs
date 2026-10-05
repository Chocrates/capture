//! Nothing content-bearing survives a purge, and the next table that does
//! fails this rather than waiting to be found by hand.
//!
//! `purge_note` edits the projection in place instead of re-folding, so every
//! derived table has to be named in it. Four have now been found surviving one
//! — `mentions`, `list_items`, `ambient` and `redact_queue` — each by somebody
//! reading the function and noticing an omission.
//!
//! This asks the database instead. It enumerates every table carrying the note
//! at runtime, so a table added next year is covered the day it is added, and
//! fails if any of them still holds a row for a destroyed note.
//!
//! Tables that legitimately keep a row are listed below with a reason. That
//! list is the point: each entry is a claim somebody made in writing, the same
//! shape as `tools/unwired-allow.txt`, and adding to it is a decision rather
//! than an omission.
//!
//! # Coverage is asserted, because the first version's was accidental
//!
//! Two holes, both of which made the check report success for a table it had
//! not looked at.
//!
//! The fixture was one capture — a tagged, dated list naming a person and a
//! project. `occurrences` is only ever filled from a `Promoted::Series`, so it
//! was empty, its `COUNT(*)` was zero, and zero reads exactly like clean. It
//! was missing from `purge_note` the whole time.
//!
//! And the enumeration asked for tables with a **`tgt`** column. `links` keys
//! the note as `src`, so the one derived table holding the note's own words was
//! never in the list — inside a check whose entire subject is derived copies of
//! those words.
//!
//! So two things are asserted now beyond cleanliness: every table is reached by
//! at least one fixture before the destruction, and a table is found whatever
//! it calls the column. A fixture that stops reaching a table is a failure
//! naming it, rather than a quiet reduction in what is checked.

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

/// Tables that hold no note id at all, and why.
///
/// A table is here because nothing in it is keyed to a note, so a destroyed
/// note cannot leave a row in it. Separate from `MAY_SURVIVE`, which is about
/// a row that may *remain*: a reason for one must not excuse the other.
const NOT_KEYED_BY_A_NOTE: &[(&str, &str)] = &[
    ("meta", "the schema version, one row, no note."),
    (
        "entities",
        "a person or project, which outlives any note naming it.",
    ),
    (
        "entity_names",
        "the spellings that reach an entity. Keyed by entity.",
    ),
    ("fold_head", "how far the fold has read. One row."),
    (
        "held",
        "events the fold could not apply, keyed by event id.",
    ),
    ("roster", "devices. Keyed by public key."),
    ("roster_origins", "origins, keyed by public key."),
    ("roster_refused", "admissions refused, keyed by event id."),
    ("device_caps", "what a device can do. Keyed by device."),
    ("settings", "settings. One row per key."),
    (
        "note_fts",
        "the full-text index, keyed by rowid through fts_map, and cleared by the purge through it.",
    ),
    ("note_fts_data", "fts5 internals."),
    ("note_fts_idx", "fts5 internals."),
    ("note_fts_docsize", "fts5 internals."),
    ("note_fts_config", "fts5 internals."),
    (
        "intents",
        "an intent, keyed by its own event id. Its cue and action name a place, \
         a branch or an instant — never a note.",
    ),
    (
        "quarantine",
        "events this device holds but cannot yet apply, keyed by event id. A \
         quarantined event is not a note and has no projection row.",
    ),
    (
        "sqlite_sequence",
        "SQLite's own bookkeeping for AUTOINCREMENT.",
    ),
];

/// Tables no fixture can fill *before* a destruction, and why.
///
/// A third list, separate from the other two on purpose. `MAY_SURVIVE` is "this
/// row may remain afterwards"; `NOT_KEYED_BY_A_NOTE` is "no note can ever be in
/// here"; this is "a row can exist, but not yet at the moment this test
/// measures". Letting any one of them excuse another is how a check stops
/// checking.
const CANNOT_PRECEDE_A_DESTRUCTION: &[(&str, &str)] = &[
    (
        "redact_queue",
        "the queued destruction itself. It appears when the redaction is \
         ordered — after this test measures — and the destruction clears it.",
    ),
    (
        "late_cancels",
        "the record that a cancellation arrived *after* the destruction. It \
         cannot exist before one by definition.",
    ),
    (
        "divergences",
        "two versions of a note that disagree, which needs a second device \
         writing concurrently. Reachable, but not from one store, and a pairing \
         fixture here would be testing sync rather than the purge.",
    ),
];

/// The column a table holds the note under, where it holds one.
///
/// `tgt` almost everywhere, `src` in `links` — which is exactly why this asks
/// rather than assuming. Keying on one name meant `links` was never enumerated,
/// and it is the derived table that holds the note's own words.
fn note_column(db: &rusqlite::Connection, table: &str) -> Option<&'static str> {
    let mut st = db
        .prepare(&format!("SELECT * FROM \"{table}\" LIMIT 0"))
        .ok()?;
    let names = st.column_names();
    let found = ["tgt", "src"].into_iter().find(|c| names.contains(c));
    let _ = st.query([]);
    found
}

/// Every table in the database, asked rather than listed.
fn all_tables(db: &rusqlite::Connection) -> Vec<String> {
    let mut st = db
        .prepare("SELECT name FROM sqlite_master WHERE type = \'table\'")
        .unwrap();
    let rows = st.query_map([], |r| r.get::<_, String>(0)).unwrap();
    let mut names: Vec<String> = rows.map(|r| r.unwrap()).collect();
    names.sort();
    names
}

/// Every table that carries a note, with the column it carries it under.
fn tables_keyed_by_a_note(db: &rusqlite::Connection) -> Vec<(String, &'static str)> {
    all_tables(db)
        .into_iter()
        .filter_map(|t| note_column(db, &t).map(|c| (t, c)))
        .collect()
}

/// Count a note's rows in every table that carries one.
fn rows_per_table(db: &rusqlite::Connection, tgt: &capture_core::EventId) -> Vec<(String, i64)> {
    tables_keyed_by_a_note(db)
        .into_iter()
        .map(|(table, col)| {
            let n: i64 = db
                .query_row(
                    &format!("SELECT COUNT(*) FROM \"{table}\" WHERE \"{col}\" = ?1"),
                    rusqlite::params![tgt.0.to_vec()],
                    |r| r.get(0),
                )
                .unwrap();
            (table, n)
        })
        .collect()
}

/// One capture, destroyed, with what it populated beforehand.
///
/// Returns the tables that held a row *before* the destruction, and those that
/// still held one after. The first is what makes coverage checkable; without it
/// a table the fixture never reached reports clean.
fn destroy_and_measure(
    tag: &str,
    text: &str,
    extra: impl FnOnce(&StorePaths, &capture_core::EventId, &str),
) -> (Vec<String>, Vec<String>) {
    let sc = Scratch::new(tag);
    let p = sc.paths();

    let slug = {
        let mut s = Session::open_for_capture(p.clone()).unwrap();
        let body = capture_store::session::capture_body(text).unwrap();
        s.capture(&body).unwrap().slug
    };
    capture_store::promote::settle(&p).unwrap();

    let tgt = capture_store::projection::current(&p)
        .unwrap()
        .resolve(&slug)
        .unwrap()
        .expect("the note is named");

    extra(&p, &tgt, &slug);
    capture_store::promote::settle(&p).unwrap();

    let before: Vec<String> = {
        let store = capture_store::projection::current(&p).unwrap();
        let db = rusqlite::Connection::open_with_flags(
            store.path(),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        rows_per_table(&db, &tgt)
            .into_iter()
            .filter(|(_, n)| *n > 0)
            .map(|(t, _)| t)
            .collect()
    };

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
    let after: Vec<String> = rows_per_table(&db, &tgt)
        .into_iter()
        .filter(|(table, n)| *n > 0 && !MAY_SURVIVE.iter().any(|(t, _)| t == table))
        .map(|(t, n)| format!("{t} ({n} row(s))"))
        .collect();

    (before, after)
}

#[test]
fn a_destroyed_note_leaves_nothing_but_the_fact_of_itself() {
    // Several captures rather than one, because no single capture can populate
    // every derived table: a list and a series are different promotions, and
    // the first version's fixture was a list — which is how `occurrences` stayed
    // empty, counted zero, and read as clean while missing from `purge_note`.
    let mut reached: std::collections::BTreeSet<String> = Default::default();
    let mut survived: Vec<String> = Vec::new();

    // A dated capture naming a person and a project, which is also tagged,
    // filed under a parent and reviewed.
    //
    // The tag is applied as an *event*: `#vault` in the text is a mention, not
    // a `tags` row — "a tag is something done to a note, and a mention is part
    // of what the note says" — so a capture alone never reaches `tags` or
    // `tag_adds`, and the first version of this fixture did not.
    let (before, after) = destroy_and_measure(
        "mentions",
        "lunch with @sarah about ^acme on 2026-11-03",
        |p, _, slug| {
            let under = {
                let mut s = Session::open_for_capture(p.clone()).unwrap();
                let body = capture_store::session::capture_body("the acme file").unwrap();
                s.capture(&body).unwrap().slug
            };
            capture_store::mutate::tag(p, slug, "vault").unwrap();
            capture_store::mutate::set_parent(p, slug, Some(&under)).unwrap();
            capture_store::review::record(p, slug, capture_core::body::Outcome::Engaged).unwrap();
        },
    );
    reached.extend(before);
    survived.extend(after);

    // An actual list. The old fixture's text claimed to be one and was not:
    // `list_of` wants the colon anchored to the tag, and
    // `#vault lunch with @sarah about ^acme on 2026-11-03: milk, eggs` is prose
    // with a colon in it. So `list_items` — one of the tables found missing
    // from the purge by hand — was never reached by the test meant to catch it.
    let (before, after) = destroy_and_measure("list", "#shop: milk, eggs, bread", |_, _, _| {});
    reached.extend(before);
    survived.extend(after);

    // A note that links to another, destroyed from the linking end, because
    // `links` is keyed by `src`.
    let (before, after) = destroy_and_measure(
        "links",
        "ask about [[settlement]] before friday",
        |p, _, _| {
            let target = {
                let mut s = Session::open_for_capture(p.clone()).unwrap();
                let body = capture_store::session::capture_body("the settlement figure").unwrap();
                s.capture(&body).unwrap().slug
            };
            capture_store::mutate::set_alias(p, &target, Some("settlement")).unwrap();
        },
    );
    reached.extend(before);
    survived.extend(after);

    // A recurring series, with one occurrence given its own details and
    // another ticked off. The `OCC_EDIT` is appended directly because nothing
    // in the system can author one — `Kind::OCC_EDIT` has a body, a fold arm
    // and three columns, and no writer in `mutate`, the CLI, the FFI, the
    // window or the phone. The columns exist, so the purge has to clear them
    // whenever they do get filled.
    let (before, after) =
        destroy_and_measure("series", "standup every weekday at 9", |p, tgt, _| {
            let mut s = Session::open(p.clone()).unwrap();
            let occ = s
                .state
                .occurrences_of(tgt)
                .unwrap()
                .first()
                .expect("a series expands")
                .occ;
            s.append(
                capture_core::event::Kind::OCC_EDIT,
                Some(*tgt),
                None,
                |nonce| {
                    minicbor::to_vec(capture_core::body::OccEditBody {
                        nonce,
                        occ,
                        title: Some(String::from("standup, moved room")),
                        location: Some(String::from("the small meeting room")),
                        notes: Some(String::from("bring the migration numbers")),
                    })
                    .expect("encoding into a Vec is infallible")
                },
            )
            .unwrap();
            s.append(
                capture_core::event::Kind::OCC_DONE,
                Some(*tgt),
                None,
                |nonce| {
                    minicbor::to_vec(capture_core::body::OccBody { nonce, occ })
                        .expect("encoding into a Vec is infallible")
                },
            )
            .unwrap();
        });
    reached.extend(before);
    survived.extend(after);

    assert!(
        survived.is_empty(),
        "a destroyed note still has rows in: {}.\n\
         Either the purge should clear them, or they belong in MAY_SURVIVE \
         with a reason saying what they keep is the note's existence rather \
         than its content.",
        survived.join(", ")
    );

    // And the part that makes the above mean anything: every table this test
    // claims to check was actually reached by one of the fixtures. A table no
    // fixture populates reports clean for ever, which is how `occurrences` hid.
    let sc = Scratch::new("coverage");
    let p = sc.paths();
    drop(Session::open(p.clone()).unwrap());
    let store = capture_store::projection::current(&p).unwrap();
    let db = rusqlite::Connection::open_with_flags(
        store.path(),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();

    let unexercised: Vec<String> = tables_keyed_by_a_note(&db)
        .into_iter()
        .map(|(t, _)| t)
        .filter(|t| !reached.contains(t))
        .filter(|t| !CANNOT_PRECEDE_A_DESTRUCTION.iter().any(|(n, _)| n == t))
        .collect();

    assert!(
        unexercised.is_empty(),
        "no fixture here ever puts a row in: {}.\n\
         These are reported clean after a destruction without having been \
         looked at. Give a fixture that populates them, or say in writing why \
         one capture cannot.",
        unexercised.join(", ")
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
    let keyed: Vec<String> = tables_keyed_by_a_note(&db)
        .into_iter()
        .map(|(t, _)| t)
        .collect();

    for (name, _) in MAY_SURVIVE {
        assert!(
            keyed.iter().any(|t| t == name),
            "MAY_SURVIVE names {name}, which is not a table carrying a note"
        );
    }

    // The same check for the other list, and the check that it is complete:
    // every table in the database either carries a note or has a written reason
    // for not carrying one. Without this, a new table keyed under a third
    // column name — `note_id`, say — would simply not be enumerated, which is
    // precisely how `links` escaped by calling its column `src`.
    for (name, _) in NOT_KEYED_BY_A_NOTE {
        assert!(
            all_tables(&db).iter().any(|t| t == name),
            "NOT_KEYED_BY_A_NOTE names {name}, which is not a table here"
        );
        assert!(
            !keyed.iter().any(|t| t == name),
            "NOT_KEYED_BY_A_NOTE names {name}, which does carry a note"
        );
    }

    let unaccounted: Vec<String> = all_tables(&db)
        .into_iter()
        .filter(|t| !keyed.contains(t))
        .filter(|t| !NOT_KEYED_BY_A_NOTE.iter().any(|(n, _)| n == t))
        .collect();
    assert!(
        unaccounted.is_empty(),
        "these tables neither carry a note under a column this test knows \
         about, nor say why they hold none: {}.\n\
         If one of them keys a note under another name, this check cannot see \
         it — which is how `links` went unchecked by calling its column `src`.",
        unaccounted.join(", ")
    );
}

#[test]
fn a_series_queued_for_destruction_still_has_its_occurrences() {
    // Written to find out, not to assert a conclusion reached in advance.
    //
    // `occurrences` is expanded from the *promotion*, not from the note's text,
    // so it sits outside the `readable_text` guard that withholds the
    // text-derived tables while a destruction is pending. The question is
    // whether that matters: the rows may exist and be unreachable, because the
    // queries a surface uses join `notes` and go through the withholding there.
    let sc = Scratch::new("queued-series");
    let p = sc.paths();

    let slug = {
        let mut s = Session::open_for_capture(p.clone()).unwrap();
        let body = capture_store::session::capture_body("standup every weekday at 9").unwrap();
        s.capture(&body).unwrap().slug
    };
    capture_store::promote::settle(&p).unwrap();

    let tgt = capture_store::projection::current(&p)
        .unwrap()
        .resolve(&slug)
        .unwrap()
        .expect("the note is named");

    let before = capture_store::projection::current(&p)
        .unwrap()
        .occurrences_of(&tgt)
        .unwrap()
        .len();
    assert!(before > 0, "a series should expand, or this proves nothing");

    capture_store::mutate::redact(&p, &slug).unwrap();

    let store = capture_store::projection::current(&p).unwrap();
    let during = store.occurrences_of(&tgt).unwrap();

    // The rows are still there: nothing withholds them, because nothing about
    // an occurrence is derived from the text the redaction is about to destroy.
    assert_eq!(
        during.len(),
        before,
        "the occurrence rows changed when the destruction was merely ordered"
    );

    // And what they carry is a schedule. The `title`, `location` and `notes`
    // columns — the only ones that could hold words somebody wrote — are empty,
    // because nothing in the system can author an `OCC_EDIT`: the kind has a
    // body, a fold arm and three columns, and no writer in `mutate`, the CLI,
    // the FFI, the window or the phone.
    //
    // So the queued window exposes when a destroyed-pending series recurs, not
    // what it says. That is worth recording as the thing to revisit the day
    // per-occurrence editing is wired up, rather than withholding now on the
    // strength of columns nothing can fill.
    assert!(
        during.iter().all(|o| o.title.is_none()),
        "an occurrence carries an edited title, so this test's reasoning is stale: \
         withhold these rows while a destruction is pending"
    );
}
