//! Reading: timeline, show, search, links.
//!
//! # Every one of these opens read-only
//!
//! None of them takes the writer lock. A diagnostic that stops working when the
//! system is unhealthy is unusable at the one moment it is needed, and "why
//! will nothing write?" is exactly the question asked while something else
//! holds the lock.
//!
//! # No note ever renders as a blank row
//!
//! A note can be redacted, muted, diverged, or simply have no readable text
//! yet. Each of those is *shown*, with a marker, rather than being an absence:
//!
//! ```text
//! budget-40k-9g9x61   [redacted 2026-08-21]
//! walk-the-dog-am3p   [muted]
//! auth-rewrite-kwgz   [2 versions]
//! ```
//!
//! Rendering any of them as an empty line would leave the reader unable to tell
//! a destroyed note from a broken one, and omitting them entirely would make the
//! timeline quietly lie about what the archive holds.
//!
//! # Nothing here counts the user
//!
//! Counts describing the data are fine — how many notes, how many quarantined
//! events. Anything that characterises the *person* is not: no streak, no
//! completion figure, no capture target. Repeated performance failure is a
//! documented mechanism behind the anxiety this system is built for, so a
//! surface that manufactures a running failure signal works against the reason
//! it exists.

use capture_store::{
    Error, Result, StorePaths,
    projection::{self, Backlink, LinkRow, NoteRow, Store},
    store,
};

use crate::json::{Json, View, answer, arr, obj};

/// Open the current projection without needing the writer.
pub fn open(paths: &StorePaths) -> Result<Store> {
    if !store::exists(paths) {
        return Err(Error::StoreMissing(paths.root.clone()));
    }
    projection::current(paths)
}

/// One line, where this answer is drawn from state nothing has caught up.
///
/// §3.1 puts the parse after the commit, so a capture is not a promotion until
/// something reads it; §8.8 puts reads off the writer, so a read cannot be the
/// thing that reads it. What is left is saying so — and an empty answer that
/// admits it is incomplete is a different statement from an empty answer, which
/// is the whole difference between a working system and one that looks working.
///
/// Read-only and deliberately so. A report of staleness that needed the writer
/// would be unavailable exactly when the writer has not been running, which is
/// the only time it matters.
///
/// Printed after the answer, never instead of it. Suppressing the output would
/// make a read fail when the sweep is behind, which is §8.8's failure with the
/// sign flipped.
fn say_if_behind(unread: usize) {
    match unread {
        0 => {}
        1 => {
            say!();
            say!("1 capture has not been read yet, so this may be incomplete.");
            say!("Read them with:  capture settle");
        }
        n => {
            say!();
            say!("{n} captures have not been read yet, so this may be incomplete.");
            say!("Read them with:  capture settle");
        }
    }
}

/// How many captures nothing has read yet, for a view to carry.
///
/// Counted when the view is built, so the prose and the JSON report the same
/// number — the JSON used to ask separately, which is how `agenda --json` came
/// to answer without it.
fn unread(store: &Store) -> usize {
    // A count that cannot be taken is not worth a warning of its own. The
    // answer beside it still stands, and a store too broken to count is
    // `capture verify`'s business, not this line's.
    store
        .unread_count(capture_parse::GRAMMAR_VERSION)
        .unwrap_or(0)
}

// ----------------------------------------------------------------- agenda ---

/// What occurs and what is due, for one day.
///
/// Answered from expanded occurrences and stored promotion fields, never by
/// reading anybody's text. Two bands, because §4.4 makes the split a
/// consequence of the time model rather than a style choice: all-day entries are
/// floating dates and have no position on an hour axis.
pub fn agenda(paths: &StorePaths, day: Option<&str>, json: bool) -> Result<()> {
    let store = open(paths)?;
    let zone = capture_time::Zone::new(&local_zone()).unwrap_or_else(utc);

    let date = match day {
        Some(d) => capture_time::FloatingDate::parse(d)
            .ok_or_else(|| Error::Usage(format!("'{d}' is not a date; use YYYY-MM-DD")))?,
        None => capture_time::civil_date(now_ms(), zone),
    };

    let from = capture_time::at_local(date, 0, 0, zone)
        .ok_or_else(|| Error::Usage("that day does not exist in this zone".into()))?;
    let to = from + 24 * 60 * 60 * 1000;

    let mut occurrences = Vec::new();

    // All-day entries first. They are floating dates with no instant to
    // compare, so they are found by date rather than by window (§3.2).
    for tgt in store.all_day_on(&format!("{date}"))? {
        occurrences.push(Occurrence {
            slug: store.slug_of(&tgt)?.unwrap_or_default(),
            o: projection::OccurrenceRow {
                occ: from,
                start_ms: from,
                end_ms: None,
                all_day: true,
                shift: None,
                cancelled: false,
                done: false,
                title: None,
                location: None,
                notes: None,
            },
            text: store.text_of(&tgt)?,
        });
    }

    for (tgt, o) in store.happening_between(from, to)? {
        // Cancelled occurrences are not drawn. They are not gone either: the
        // cancellation is an ordinary event and nothing was destroyed (§4.4).
        // One that was also moved *is* drawn — a move outranks a cancel.
        if o.cancelled && o.end_ms.is_none() && o.start_ms == o.occ {
            continue;
        }
        occurrences.push(Occurrence {
            slug: store.slug_of(&tgt)?.unwrap_or_default(),
            o,
            text: store.text_of(&tgt)?,
        });
    }

    // Tasks due on this day. §4.4's agenda is "what is due and what occurs",
    // and until this existed it was only the second half — a task due today was
    // in `capture tasks`, and invisible on the day it mattered.
    //
    // In the all-day band, because a due date has no hour and putting one on it
    // would invent a deadline nobody wrote.
    let mut due = Vec::new();
    for (tgt, done) in store.tasks_due_on(&format!("{date}"))? {
        due.push(DueTask {
            slug: store.slug_of(&tgt)?.unwrap_or_default(),
            text: store.text_of(&tgt)?,
            done,
        });
    }

    // Overdue tasks, on today only. Still open rather than lapsed: §3.6 draws
    // that line, and a task is the one thing here that genuinely carries
    // forward — it is not finished, and saying so once is not a backlog.
    let today = date == capture_time::civil_date(now_ms(), zone);
    let mut overdue = Vec::new();
    if today {
        for tgt in store.tasks_overdue(&format!("{date}"))? {
            let was_due = store
                .promotion_of(&tgt)?
                .and_then(|p| match p.as_ {
                    capture_core::body::Promoted::Task(t) => t.due,
                    _ => None,
                })
                .unwrap_or_default();
            overdue.push(Overdue {
                slug: store.slug_of(&tgt)?.unwrap_or_default(),
                text: store.text_of(&tgt)?,
                due: was_due,
            });
        }
    }

    // What is still owed, from series that carry forward. Shown on today only:
    // a debt is a thing about now, not something to redraw on every past day
    // somebody scrolls back through.
    let mut owed = Vec::new();
    if today {
        for (tgt, o) in store.outstanding(from)? {
            owed.push(Occurrence {
                slug: store.slug_of(&tgt)?.unwrap_or_default(),
                o,
                text: store.text_of(&tgt)?,
            });
        }
    }

    answer(
        &Agenda {
            date: format!("{date}"),
            from,
            zone,
            occurrences,
            due,
            overdue,
            owed,
            unread: unread(&store),
        },
        json,
    );
    Ok(())
}

/// One day: what occurs on it, what is due, and — today only — what carries in.
struct Agenda {
    date: String,
    /// The day's first instant, which is where an all-day row sits.
    from: i64,
    zone: capture_time::Zone,
    occurrences: Vec<Occurrence>,
    due: Vec<DueTask>,
    overdue: Vec<Overdue>,
    owed: Vec<Occurrence>,
    unread: usize,
}

/// An occurrence, with the note it belongs to.
struct Occurrence {
    slug: String,
    o: projection::OccurrenceRow,
    text: Option<String>,
}

/// A task due on the day.
struct DueTask {
    slug: String,
    text: Option<String>,
    done: bool,
}

/// A task due on an earlier day and still open.
struct Overdue {
    slug: String,
    text: Option<String>,
    due: String,
}

impl Occurrence {
    /// What the occurrence says about itself, beside what the note says.
    ///
    /// The prose picks `title` over the note's text when there is one; the JSON
    /// once emitted only the text, so an amendment — "this occurrence only:
    /// different title, location, notes" — was visible in a terminal and
    /// invisible to anything reading the JSON.
    fn json(&self) -> Json {
        let Occurrence { slug, o, text } = self;
        let projection::OccurrenceRow {
            occ: _, // the rule's instant; `start_ms` is where it is shown
            start_ms,
            end_ms,
            all_day,
            shift,
            cancelled,
            done,
            title,
            location,
            notes,
        } = o;
        obj([
            ("slug", slug.into()),
            ("start_ms", start_ms.into()),
            ("end_ms", (*end_ms).into()),
            ("all_day", (*all_day).into()),
            ("done", (*done).into()),
            ("cancelled", (*cancelled).into()),
            ("shift", shift.into()),
            ("due", false.into()),
            ("text", text.into()),
            ("title", title.into()),
            ("location", location.into()),
            ("notes", notes.into()),
        ])
    }
}

impl View for Agenda {
    fn json(&self) -> Json {
        let Agenda {
            date,
            from,
            zone: _, // the instants are absolute; the zone only draws them
            occurrences,
            due,
            overdue,
            owed,
            unread,
        } = self;
        let mut entries: Vec<Json> = occurrences.iter().map(Occurrence::json).collect();
        // Tasks carry `due: true` rather than being a separate array, because a
        // caller asking "what is this day" wants one list — and one that
        // silently omitted what is due would be answering a narrower question
        // than it was asked. The same fields as an occurrence, so a consumer
        // reads every entry one way.
        entries.extend(due.iter().map(|DueTask { slug, text, done }| {
            obj([
                ("slug", slug.into()),
                ("start_ms", from.into()),
                ("end_ms", Json::Null),
                ("all_day", true.into()),
                ("done", (*done).into()),
                ("cancelled", false.into()),
                ("shift", Json::Null),
                ("due", true.into()),
                ("text", text.into()),
                ("title", Json::Null),
                ("location", Json::Null),
                ("notes", Json::Null),
            ])
        }));
        // An object rather than a bare array, so the count can ride along — a
        // consumer reading an empty list as "nothing that day" was being told
        // something that might not be true. The agenda has the stronger case:
        // a day with nothing on it is a far commoner answer than an empty task
        // list, and a script is the consumer least able to notice.
        //
        // Present at zero deliberately. A field that appeared only when
        // something was unread would make the shape depend on the state, so a
        // consumer that worked all week would break the first morning the sweep
        // fell behind. `overdue` and `owed` are present and empty on any day
        // but today for the same reason.
        obj([
            ("date", date.into()),
            ("entries", Json::Arr(entries)),
            (
                "overdue",
                arr(overdue.iter().map(|Overdue { slug, text, due }| {
                    obj([
                        ("slug", slug.into()),
                        ("text", text.into()),
                        ("due", due.into()),
                    ])
                })),
            ),
            ("owed", arr(owed.iter().map(Occurrence::json))),
            ("unread", unread.into()),
        ])
    }

    fn prose(&self) {
        let Agenda {
            date,
            zone,
            occurrences,
            due,
            overdue,
            owed,
            unread,
            ..
        } = self;
        let zone = *zone;

        say!("{date}");
        if occurrences.is_empty() && due.is_empty() && overdue.is_empty() && owed.is_empty() {
            say!("  nothing");
            say_if_behind(*unread);
            return;
        }

        for d in due {
            say!(
                "  {}  {}  {}",
                if d.done { "done    " } else { "due     " },
                d.slug,
                d.text.as_deref().map(first_line).unwrap_or_default()
            );
        }

        // All-day first, in their own band. They have no hour to sort by, and
        // mixing them into the timed list would imply one.
        for r in occurrences.iter().filter(|r| r.o.all_day) {
            say!("  all day   {}  {}", r.slug, line(r.text.as_deref(), &r.o));
        }
        for r in occurrences.iter().filter(|r| !r.o.all_day) {
            let (h, m) = capture_time::civil_clock(r.o.start_ms, zone);
            say!(
                "  {h:02}:{m:02}     {}  {}",
                r.slug,
                line(r.text.as_deref(), &r.o)
            );
        }

        if !overdue.is_empty() {
            say!();
            say!("Still open");
            for t in overdue {
                say!(
                    "  was due {}  {}  {}",
                    t.due,
                    t.slug,
                    t.text.as_deref().map(first_line).unwrap_or_default()
                );
            }
        }

        if !owed.is_empty() {
            say!();
            say!("Still owed");
            for r in owed {
                let d = capture_time::civil_date(r.o.start_ms, zone);
                say!(
                    "  from {d}  {}  {}",
                    r.slug,
                    r.text.as_deref().map(first_line).unwrap_or_default()
                );
            }
        }
        say_if_behind(*unread);
    }
}

/// One agenda line's text, with whatever the occurrence says about itself.
fn line(text: Option<&str>, o: &projection::OccurrenceRow) -> String {
    let body = o
        .title
        .clone()
        .or_else(|| text.map(first_line))
        .unwrap_or_else(|| "(no readable text)".into());

    let mut s = body;
    if o.done {
        s.push_str("  ✓");
    }
    // A shifted occurrence says so. §4.4: a time that moved on its own with no
    // explanation is what makes somebody stop trusting the whole system.
    match o.shift.as_deref() {
        Some("gap") => s.push_str("  (moved by a daylight-saving gap)"),
        Some("ambiguous") => s.push_str("  (the clock struck this hour twice)"),
        _ => {}
    }
    if o.cancelled {
        s.push_str("  (cancelled, and moved)");
    }
    s
}

/// Notes promoted to tasks.
pub fn tasks(paths: &StorePaths, json: bool) -> Result<()> {
    let store = open(paths)?;
    // `tasks_outstanding`, not `promoted("task", ..)`. The latter is every task
    // promotion ever made, flat and in no band — so this command listed
    // finished tasks ("done · due 2026-09-20") and had none of the three bands
    // the requirement names, while the query that does both sat beside it with
    // its boundaries property-tested.
    //
    // Found by completing a task and looking, rather than by reading: the
    // store's own test asserts a finished task leaves `tasks_outstanding`, and
    // this command was not asking `tasks_outstanding`.
    // Today in the reading zone, which is what the overdue boundary is defined
    // against: a task due today is not overdue.
    let zone = capture_time::Zone::new(&local_zone()).unwrap_or_else(utc);
    let today = format!("{}", capture_time::civil_date(now_ms(), zone));

    let mut rows = Vec::new();
    for row in store.tasks_outstanding(&today)? {
        rows.push(Task {
            slug: store.slug_of(&row.tgt)?.unwrap_or_default(),
            text: store.text_of(&row.tgt)?,
            row,
        });
    }

    answer(
        &Tasks {
            rows,
            unread: unread(&store),
        },
        json,
    );
    Ok(())
}

/// Every outstanding task, banded.
struct Tasks {
    rows: Vec<Task>,
    unread: usize,
}

/// One task, with the note it is.
struct Task {
    slug: String,
    text: Option<String>,
    row: capture_store::projection::TaskRow,
}

impl View for Tasks {
    fn json(&self) -> Json {
        let Tasks { rows, unread } = self;
        let items = rows.iter().map(|Task { slug, text, row }| {
            let capture_store::projection::TaskRow {
                tgt: _, // named by `slug`
                descriptor,
                due,
                band,
            } = row;
            // `band` and `due` ride along: a consumer handed only a descriptor
            // has to parse prose to learn when something is due, and cannot
            // tell overdue from merely open at all.
            obj([
                ("slug", slug.into()),
                ("descriptor", descriptor.into()),
                ("band", band_name(*band).into()),
                ("due", due.into()),
                ("text", text.into()),
            ])
        });
        // An object rather than the bare array it used to be, so the count can
        // ride along. A consumer reading an empty list as "no tasks" was being
        // told something that might not be true.
        obj([("tasks", arr(items)), ("unread", unread.into())])
    }

    fn prose(&self) {
        let Tasks { rows, unread } = self;
        if rows.is_empty() {
            say!("Nothing outstanding.");
            say!();
            say!("A capture becomes one on its own where a verb and a day agree —");
            say!("\"call the dentist tomorrow\". A verb with no day is a suggestion");
            say!("and nothing more, so say it yourself with: capture task <name> [due]");
            say_if_behind(*unread);
            return;
        }
        // Banded, in the order the requirement states, and the undated band
        // drawn rather than folded into the others: a task with no due date is
        // the ordinary case, because a bare task verb is below the confidence
        // floor and somebody saying "send sarah the migration doc" is not
        // saying when.
        for (band, label) in [
            (capture_store::projection::Band::Overdue, "overdue"),
            (capture_store::projection::Band::Due, "due"),
            (capture_store::projection::Band::Undated, "no date"),
        ] {
            let mine: Vec<&Task> = rows.iter().filter(|t| t.row.band == band).collect();
            if mine.is_empty() {
                continue;
            }
            say!("{label}");
            for t in mine {
                say!(
                    "  {:<28}  {:<22}  {}",
                    t.slug,
                    t.row.descriptor,
                    t.text.as_deref().map(first_line).unwrap_or_default()
                );
            }
        }
        say_if_behind(*unread);
    }
}

/// The band, as the one word every surface uses for it.
fn band_name(b: capture_store::projection::Band) -> &'static str {
    match b {
        capture_store::projection::Band::Overdue => "overdue",
        capture_store::projection::Band::Due => "due",
        capture_store::projection::Band::Undated => "undated",
    }
}

/// Enough of an id to name one plan, the way a short commit hash does.
fn short(id: &[u8]) -> String {
    id.iter().take(4).map(|b| format!("{b:02x}")).collect()
}

/// Every note naming a person or a project, or carrying a tag.
///
/// §4.3's entity page, in a terminal. One verb for the three sigils: they come
/// from different tables and mean different things underneath — a tag is an
/// OR-Set with its own events, a mention is part of the text — and none of that
/// is the reader's to keep track of while following one.
///
/// The sigil is required rather than guessed. `capture mentions sarah` could mean
/// the person, the project or the tag, and picking one silently would be the
/// kind of helpfulness that returns the wrong answer confidently.
pub fn mentions(paths: &StorePaths, name: &str, json: bool) -> Result<()> {
    let store = open(paths)?;
    let mut chars = name.chars();
    let sigil = chars.next().unwrap_or(' ');
    let value: String = chars.collect();

    if value.is_empty() || !matches!(sigil, '@' | '^' | '#') {
        return Err(Error::Usage(format!(
            "'{name}' is not a mention; use @person, ^project or #tag"
        )));
    }

    let rows = match sigil {
        '#' => store.tagged(&value, now_ms() as u64)?,
        _ => store.mentioning(sigil, &value, now_ms() as u64)?,
    };

    answer(
        &Mentions {
            name: name.to_string(),
            notes: rows.iter().map(Named::of).collect(),
        },
        json,
    );
    Ok(())
}

/// A note as a listing names it: what it is called and what it says.
struct Named {
    slug: String,
    text: Option<String>,
}

impl Named {
    fn of(n: &NoteRow) -> Named {
        Named {
            slug: n.slug.clone(),
            text: n.text.clone(),
        }
    }

    fn json(&self) -> Json {
        let Named { slug, text } = self;
        obj([("slug", slug.into()), ("text", text.into())])
    }

    /// One indented line, the name padded to `width`.
    fn say(&self, width: usize) {
        say!(
            "  {:<width$}  {}",
            self.slug,
            self.text.as_deref().map(first_line).unwrap_or_default()
        );
    }
}

/// Every note naming one thing.
struct Mentions {
    /// What was asked for, as it was written.
    name: String,
    notes: Vec<Named>,
}

impl View for Mentions {
    fn json(&self) -> Json {
        // `name` is the question, not part of the answer.
        let Mentions { name: _, notes } = self;
        arr(notes.iter().map(Named::json))
    }

    fn prose(&self) {
        let Mentions { name, notes } = self;
        if notes.is_empty() {
            say!("Nothing names {name}.");
            say!();
            say!("Pages are not created — they exist for whatever your notes say.");
            say!("Write {name} in a capture and this one fills itself in.");
            return;
        }

        say!("{name}");
        say!();
        for n in notes {
            n.say(40);
        }
    }
}

/// A written mention split into the kind it names and the name itself.
///
/// The sigil's meaning comes from `capture-core`, beside `EntId::derive`,
/// which takes the kind this returns. Those two together are the whole of how
/// a written sigil becomes an identity; this file only decides what a refusal
/// reads like on a command line.
pub(crate) fn split_mention(name: &str) -> Result<(&'static str, String)> {
    let mut chars = name.chars();
    let sigil = chars.next().unwrap_or(' ');
    let value: String = chars.collect();
    let Some(kind) = capture_core::id::entity_kind(sigil) else {
        return Err(Error::Usage(format!(
            "'{name}' is not a mention; use @person, ^project or #tag"
        )));
    };
    if value.is_empty() {
        return Err(Error::Usage(format!(
            "'{name}' names nothing after the sigil"
        )));
    }
    Ok((kind, value))
}

/// Everyone, or every project, the archive knows.
///
/// The index `distinct_mentions` could answer and nothing ever asked for. Sorted
/// by how much is said about each, because that is the order somebody scanning
/// for a name they half-remember is scanning in.
pub fn entities(paths: &StorePaths, kind: &str, json: bool) -> Result<()> {
    let store = open(paths)?;
    let mut rows = store.entities_of_kind(kind, now_ms() as u64)?;
    rows.sort_by(|a, b| b.count.cmp(&a.count).then(a.display.cmp(&b.display)));
    answer(
        &Entities {
            kind: kind.to_string(),
            rows,
        },
        json,
    );
    Ok(())
}

/// Every entity of one kind.
struct Entities {
    kind: String,
    rows: Vec<projection::EntityRow>,
}

impl View for Entities {
    fn json(&self) -> Json {
        // `kind` is the question; every row is of it.
        let Entities { kind: _, rows } = self;
        arr(rows.iter().map(|e| {
            let projection::EntityRow {
                ent: _,  // an internal id; `display` and `names` reach it
                kind: _, // the same for every row
                display,
                backing: _, // a page's business, not an index's
                names,
                count,
            } = e;
            obj([
                ("display", display.into()),
                ("count", count.into()),
                ("names", arr(names)),
            ])
        }))
    }

    fn prose(&self) {
        let Entities { kind, rows } = self;
        if rows.is_empty() {
            let (what, sigil) = if kind == "person" {
                ("people", "@")
            } else {
                ("projects", "^")
            };
            say!("No {what} yet.");
            say!();
            say!("They are not created — one exists as soon as a note names it.");
            say!("Write {sigil}somebody in a capture and it appears here.");
            return;
        }

        for e in rows {
            let also = if e.names.len() > 1 {
                format!("   (also {})", e.names.join(", "))
            } else {
                String::new()
            };
            say!("  {:<30}  {:>4} note(s){}", e.display, e.count, also);
        }
    }
}

/// One entity's page.
///
/// What it is called, every spelling that reaches it, the note that *is* it
/// where one is bound, and then the notes naming it. The backing note is printed
/// above rather than in the list, because a row appearing twice reads as two
/// notes.
pub fn entity(paths: &StorePaths, name: &str, json: bool) -> Result<()> {
    let store = open(paths)?;
    let (kind, value) = split_mention(name)?;
    let page = match store.entity(kind, &value)? {
        None => None,
        Some(ent) => {
            let notes = store.entity_notes(&ent.ent, now_ms() as u64)?;
            let backing = match ent.backing {
                Some(tgt) => store.slug_of(&tgt)?,
                None => None,
            };
            Some(EntityPage {
                display: ent.display,
                kind: ent.kind,
                backing,
                names: ent.names,
                notes: notes.iter().map(Named::of).collect(),
            })
        }
    };
    answer(
        &Entity {
            name: name.to_string(),
            value,
            page,
        },
        json,
    );
    Ok(())
}

/// An entity's page, or the absence of one.
struct Entity {
    /// What was asked for, sigil and all.
    name: String,
    /// The same without its sigil, to tell whether it is the only spelling.
    value: String,
    page: Option<EntityPage>,
}

struct EntityPage {
    display: String,
    kind: String,
    /// The note that is this entity, by name.
    backing: Option<String>,
    names: Vec<String>,
    notes: Vec<Named>,
}

impl View for Entity {
    fn json(&self) -> Json {
        // The question, twice over; the page is the answer.
        let Entity {
            name: _,
            value: _,
            page,
        } = self;
        let Some(EntityPage {
            display,
            kind,
            backing,
            names,
            notes,
        }) = page
        else {
            return Json::Null;
        };
        obj([
            ("display", display.into()),
            ("kind", kind.into()),
            ("backing", backing.into()),
            ("names", arr(names)),
            ("notes", arr(notes.iter().map(Named::json))),
        ])
    }

    fn prose(&self) {
        let Entity { name, value, page } = self;
        let Some(p) = page else {
            say!("Nothing names {name}.");
            say!();
            say!("Pages are not created — they exist for whatever your notes say.");
            say!("Write {name} in a capture and this one fills itself in.");
            return;
        };

        say!("{}", p.display);
        if p.names.len() > 1 || p.names.first() != Some(value) {
            say!("  also: {}", p.names.join(", "));
        }
        if let Some(slug) = &p.backing {
            say!();
            say!("  the note that is this {}: {slug}", p.kind);
        }
        say!();
        if p.notes.is_empty() {
            say!("  Nothing else names it now.");
            return;
        }
        for n in &p.notes {
            n.say(40);
        }
    }
}

/// A bounded review session.
///
/// Five items, then it is over and says so. An unbounded queue is a backlog and
/// §3.6 spends its length on why a manufactured one is the wrong shape here — a
/// session you always finish cannot become one.
///
/// Nothing is scored. The session records whether an item appeared, whether you
/// did anything with it, and whether you pushed it away, and nothing anywhere
/// aggregates those into a number about you (§2.7).
pub fn review(paths: &StorePaths, json: bool) -> Result<()> {
    // A session opens the store because being offered an item is itself
    // recorded — `shown` is an outcome, and a surface that read without writing
    // would schedule the same five notes forever.
    let mut s = crate::read::session(paths.clone())?;
    let now = now_ms();
    let items = capture_store::review::session(&s, now)?;

    // One prompt, not an inference. §3.6: a series that has lapsed repeatedly
    // might be one that genuinely accumulates — a dose rather than a walk — and
    // the recognizer must never decide that from phrasing. Asking once is the
    // difference between noticing and guessing.
    //
    // Asked for the JSON too. It was skipped there, so a script driving review
    // never learned the question existed.
    let mut lapsed = Vec::new();
    for tgt in s.state.repeatedly_lapsed(now, 3)? {
        lapsed.push(s.state.slug_of(&tgt)?.unwrap_or_default());
    }

    let view = Review {
        items: items
            .iter()
            .map(|i| Offered {
                slug: i.note.slug.clone(),
                because: i.because,
                text: i.note.text.clone(),
                // What the archive suspects but would not act on. Computed in
                // `review()` and, for a while, printed nowhere: a suggestion
                // nobody is shown is a suggestion that does not exist.
                suggestion: i.note.text.as_deref().and_then(|text| {
                    capture_store::review::suggestion_for(text, i.note.tz.as_deref(), now)
                }),
            })
            .collect(),
        lapsed,
    };
    answer(&view, json);

    // Being offered is itself the outcome `shown`. Recorded after the items are
    // printed, so a failure to write cannot lose the session somebody is
    // already reading.
    let tgts: Vec<_> = items.iter().map(|i| i.note.tgt).collect();
    capture_store::review::shown(&mut s, &tgts)?;
    Ok(())
}

/// One session: what is offered, and which series keep lapsing.
struct Review {
    items: Vec<Offered>,
    /// Series that have lapsed often enough to ask whether they accumulate.
    lapsed: Vec<String>,
}

struct Offered {
    slug: String,
    because: capture_store::review::Because,
    text: Option<String>,
    suggestion: Option<String>,
}

impl View for Review {
    fn json(&self) -> Json {
        let Review { items, lapsed } = self;
        // An object rather than the bare array it was, because `lapsed` is
        // about the session and not about any one item in it.
        obj([
            (
                "items",
                arr(items.iter().map(
                    |Offered {
                         slug,
                         because,
                         text,
                         suggestion,
                     }| {
                        obj([
                            ("slug", slug.into()),
                            (
                                "because",
                                match because {
                                    capture_store::review::Because::Forward => "forward",
                                    capture_store::review::Because::Never => "never-engaged",
                                    capture_store::review::Because::Context => "context",
                                }
                                .into(),
                            ),
                            ("text", text.into()),
                            ("suggestion", suggestion.into()),
                        ])
                    },
                )),
            ),
            ("lapsed", arr(lapsed)),
        ])
    }

    fn prose(&self) {
        let Review { items, lapsed } = self;
        if items.is_empty() {
            say!("Nothing to review.");
            say!();
            say!("That is the ordinary state. Review offers what is due, and when");
            say!("nothing is, the session is over before it started.");
        } else {
            say!("{} note(s), and then this is over.", items.len());
            say!();
            for i in items {
                let why = match i.because {
                    capture_store::review::Because::Forward => "you said to come back to this",
                    capture_store::review::Because::Never => "never looked at since capture",
                    capture_store::review::Because::Context => "captured somewhere like here",
                };
                say!("  {}", i.slug);
                say!(
                    "    {}",
                    i.text.as_deref().map(first_line).unwrap_or_default()
                );
                say!("    — {why}");
                if let Some(said) = &i.suggestion {
                    say!("    {said}");
                }
                say!();
            }
            say!("capture engaged <name>   did something with it");
            say!("capture deferred <name>  push it away (twice and it stops being offered)");
        }

        for slug in lapsed {
            say!();
            say!("  {slug} has lapsed several times.");
            say!("  If missing it leaves something owed — a dose, an invoice — say so:");
            say!("    capture carries-forward {slug}");
            say!("  Otherwise there is nothing to do. A missed day costs almost nothing.");
        }
    }
}

/// Show what the recognizer makes of some text.
///
/// Stores nothing and decides nothing. It exists because the confidence bands
/// were reasoned rather than measured (§3.1.5), and the only honest way to find
/// out whether they are right is to point them at real captures and look — a
/// band that is wrong is invisible from the inside.
pub fn explain(text: &str) -> Result<()> {
    let zone = capture_time::Zone::new(&local_zone()).unwrap_or_else(utc);
    let r = capture_parse::recognize(text, now_ms(), zone);

    say!("{text}");
    say!();

    let shape = match &r.shape {
        capture_parse::Shape::Nothing => "nothing temporal".to_string(),
        capture_parse::Shape::Task { due: None } => "a task, by its verb".to_string(),
        capture_parse::Shape::Task { due: Some(d) } => format!("a task, due {d}"),
        capture_parse::Shape::Point(capture_parse::When::Date(d)) => format!("a day: {d}"),
        capture_parse::Shape::Point(capture_parse::When::Moment(ms)) => {
            let (h, m) = capture_time::civil_clock(*ms, zone);
            format!(
                "a moment: {} at {h:02}:{m:02}",
                capture_time::civil_date(*ms, zone)
            )
        }
        capture_parse::Shape::Range { from, to } => format!("a span: {from} to {to}"),
        capture_parse::Shape::Recurring { rule, at_minute } => match at_minute {
            Some(m) => format!(
                "a series: freq {} at {:02}:{:02}",
                rule.freq,
                m / 60,
                m % 60
            ),
            None => format!("a series: freq {}", rule.freq),
        },
    };

    say!("  reads as   {shape}");
    say!(
        "  confidence {}%{}",
        r.confidence,
        if r.confidence >= capture_store::promote::FLOOR {
            "  — enough to decide on its own"
        } else if r.confidence > 0 {
            "  — under the bar; a suggestion, applied to nothing"
        } else {
            ""
        }
    );

    if r.markers.any() {
        let mut parts = Vec::new();
        for t in &r.markers.tags {
            parts.push(format!("#{t}"));
        }
        for p in &r.markers.people {
            parts.push(format!("@{p}"));
        }
        if let Some(d) = &r.markers.date {
            parts.push(format!(">{d}"));
        }
        if let Some(p) = r.markers.priority {
            parts.push(format!("!p{p}"));
        }
        if let Some(m) = r.markers.duration_min {
            parts.push(format!("~{m}m"));
        }
        if let Some(p) = &r.markers.project {
            parts.push(format!("^{p}"));
        }
        say!("  markers    {}", parts.join(" "));
    }

    Ok(())
}

/// Notes captured while a recurring thing was happening.
///
/// Activation (§3.9): a meeting surfacing what was said at previous ones. The
/// association is derived from when things happened rather than filed by hand —
/// the capture carries its own time and the series says when it was on, so
/// nothing has to be maintained and nothing can go stale.
pub fn during(paths: &StorePaths, name: &str, json: bool) -> Result<()> {
    let store = open(paths)?;
    let row = find(&store, name)?;
    let notes = store.captured_during(&row.tgt, now_ms(), 200)?;
    answer(
        &During {
            notes,
            now: now_ms() as u64,
        },
        json,
    );
    Ok(())
}

/// Notes captured while something was on.
struct During {
    notes: Vec<NoteRow>,
    now: u64,
}

impl View for During {
    fn json(&self) -> Json {
        let During { notes, now } = self;
        arr(notes.iter().map(|n| note(n, *now)))
    }

    fn prose(&self) {
        let During { notes, .. } = self;
        if notes.is_empty() {
            say!("Nothing was captured while that was on.");
            say!();
            say!("Either it has not happened yet, or nothing was written down during it.");
            return;
        }

        say!("Captured while this was happening:");
        say!();
        for n in notes {
            Named::of(n).say(30);
        }
    }
}

/// Every if-then plan still standing.
pub fn intents(paths: &StorePaths, json: bool) -> Result<()> {
    let store = open(paths)?;
    answer(
        &Intents {
            rows: store.intents()?,
            zone: local_zone(),
        },
        json,
    );
    Ok(())
}

struct Intents {
    rows: Vec<projection::IntentRow>,
    /// The zone a time cue is drawn in.
    zone: String,
}

impl View for Intents {
    fn json(&self) -> Json {
        // The zone only draws `at`, which is an instant.
        let Intents { rows, zone: _ } = self;
        arr(rows.iter().map(|i| {
            let projection::IntentRow {
                id,
                cue_type,
                cue_match,
                cue_at,
                then_do,
            } = i;
            obj([
                ("id", short(id).into()),
                ("cue", cue_type.into()),
                ("match", cue_match.into()),
                ("at", (*cue_at).into()),
                ("then", then_do.into()),
            ])
        }))
    }

    fn prose(&self) {
        let Intents { rows, zone } = self;
        if rows.is_empty() {
            say!("No plans.");
            say!();
            say!("  capture intent at 2026-09-01T09:00  ring the dentist");
            say!("  capture intent branch main          run the migration check");
            return;
        }

        for i in rows {
            let when = match (i.cue_type.as_str(), &i.cue_match, i.cue_at) {
                // In this machine's zone, not UTC. A time cue stores no zone of
                // its own — `capture intent at 2026-10-06T09:00` resolves the
                // wall clock against the local zone and keeps the instant — so
                // UTC here answered a question nobody asked: somebody who typed
                // 09:00 was told their plan fires at 14:00. The same mistake
                // the agenda made.
                ("time", _, Some(at)) => format!("at {}", stamp(at as u64, Some(zone))),
                ("git_branch", Some(b), _) => format!("on branch {b}"),
                (t, Some(m), _) => format!("{t} {m}"),
                (t, None, _) => t.to_string(),
            };
            // The id, because `capture forget` needs something to name and the
            // whole one is thirty-two bytes.
            say!("  {}  {:<30}  {}", short(&i.id), when, i.then_do);
        }
    }
}

fn utc() -> capture_time::Zone {
    capture_time::Zone::new("UTC").expect("UTC is always present")
}

/// Open the store for writing, as of this command's moment.
///
/// One helper rather than nineteen `Session::open` calls, because the moment
/// is a decision and it was being made nineteen times by default.
///
/// A command is one instant. Every read already works that way — `now_ms` is
/// the single clock behind the agenda, the task bands and the due queue — and
/// the writing side did not, so a command could stamp an event at one moment
/// and decide a deadline at another. In a release build the difference is
/// microseconds; in a debug build it is the difference between being able to
/// test a deadline and not, because the work a deadline triggers runs during
/// `Session::open` itself (§8.7) and nothing after it can move time.
pub(crate) fn session(paths: capture_store::StorePaths) -> Result<capture_store::session::Session> {
    capture_store::session::Session::open_at(paths, now_ms().max(0) as u64)
}

/// The zone this machine is in, by name.
///
/// One definition, in `capture-store`'s notifier, because the only reason
/// anything here reads a zone is to work out what hour it is for the rules in
/// §2.6 — and a wrong answer moves quiet hours by the UTC offset, which is the
/// difference between a notification at 07:00 and one at 02:00.
///
/// There were four copies. The one in the overlay was made by reading the one
/// twelve lines away in this file, which is the mechanism rather than the
/// accident.
pub(crate) fn local_zone() -> String {
    capture_store::notifier::local_zone()
}

/// The clock, read in one place.
///
/// Fourteen call sites read it directly before this, which is fourteen chances
/// to read it differently — and two of them already differed about whether the
/// answer was an `i64` or a `u64`. The same argument as one `peer_is_stale`:
/// "what time is it" answered once cannot be answered two ways.
///
/// # What this clock decides, and what it does not
///
/// Measured rather than assumed, because the first version of this comment got
/// it wrong. This is the clock the command line *interprets* with: what "today"
/// is for the agenda and the overdue bands, where a `split` boundary falls, how
/// long ago a peer was reached. It is **not** what stamps events — the store
/// keeps its own clock for that (`session::now_ms`), and moving this one leaves
/// the HLC alone, as a capture made with `CAPTURE_NOW_MS` set to 2030 and
/// landing on today's timeline demonstrates.
///
/// So it also does *not* move the drain: whether a redaction's window has
/// passed is the store's question, asked with the store's clock. Anything
/// needing that — `capture recover`, snapshot expiry, the destruction itself —
/// is still out of reach, and moving the store's clock is a larger decision
/// than this, because that one would reach the stamps.
///
/// # Why the override is not in the binary you install
///
/// The precedent is `CAPTURE_PURGE_PAUSE_MS`, which ships unguarded because a
/// sleep is harmless. This is not quite that harmless: a wrong "now" can put a
/// series split at a boundary nobody chose, and that writes an event.
///
/// Nothing here can corrupt the ordering of an archive, which the first draft
/// of this comment claimed. It can still make a command do the wrong thing
/// while looking right, and `cfg(debug_assertions)` costs nothing: a test runs
/// the debug binary — that is what `CARGO_BIN_EXE_capture` is — and an
/// installed release ignores the variable because the code reading it is not
/// compiled.
pub(crate) fn now_ms() -> i64 {
    #[cfg(debug_assertions)]
    if let Some(ms) = std::env::var("CAPTURE_NOW_MS")
        .ok()
        .and_then(|v| v.parse::<i64>().ok())
    {
        return ms.max(0);
    }
    jiff::Timestamp::now().as_millisecond().max(0)
}

/// Find a note by name, or say so.
pub(crate) fn find(store: &Store, name: &str) -> Result<NoteRow> {
    let Some(tgt) = store.resolve(name)? else {
        return Err(Error::NotFound(name.to_string()));
    };
    store
        .note(&tgt)?
        .ok_or_else(|| Error::NotFound(name.to_string()))
}

// ------------------------------------------------------------- timeline ----

/// Notes newest first, grouped by the day they were captured.
///
/// The day comes from the timezone recorded *at capture*, not the reader's.
/// Grouping by the reader's zone would move notes between days when travelling,
/// which is precisely when situational cues matter most and when a note is
/// hardest to find by content.
pub fn timeline(paths: &StorePaths, limit: Option<usize>, json: bool) -> Result<()> {
    let store = open(paths)?;
    // An explicit count beats the setting, which beats the compiled-in default.
    let limit = match limit {
        Some(n) => n,
        None => crate::ops::effective(&store, "timeline.limit")?
            .and_then(|v| v.parse().ok())
            .unwrap_or(50),
    };
    answer(
        &Timeline {
            notes: store.timeline(limit)?,
            now: now_ms() as u64,
        },
        json,
    );
    Ok(())
}

struct Timeline {
    notes: Vec<NoteRow>,
    /// What "muted" is decided against: a mute with an end lapses.
    now: u64,
}

impl View for Timeline {
    fn json(&self) -> Json {
        let Timeline { notes, now } = self;
        arr(notes.iter().map(|n| note(n, *now)))
    }

    fn prose(&self) {
        let Timeline { notes, now } = self;
        if notes.is_empty() {
            empty_state();
            return;
        }

        // One column width for the whole listing, from the longest name
        // actually present. A fixed width either wraps on long names or wastes
        // half the terminal on short ones, and slugs vary by more than either
        // allows for.
        let width = name_width(notes);

        let mut current_day = String::new();
        for n in notes {
            let day = day_of(n);
            if day != current_day {
                if !current_day.is_empty() {
                    say!();
                }
                say!("{day}");
                current_day = day;
            }
            say!("  {}", row(n, width, *now));
        }
    }
}

/// The column width a listing needs, bounded so one long name cannot push every
/// line off the screen.
fn name_width(notes: &[NoteRow]) -> usize {
    notes
        .iter()
        .map(|n| n.slug.chars().count())
        .max()
        .unwrap_or(20)
        .min(44)
}

/// What to show when there is nothing yet.
///
/// Names the gesture that actually works on *this* machine rather than the one
/// the design prefers. Advertising a chord this desktop cannot register would
/// be the worst possible first instruction: it fails silently, and the person
/// is left believing the tool is broken rather than that a different path
/// exists.
///
/// No sample content. A seeded note is a note somebody has to identify as
/// unreal and then delete, and it makes the first genuine capture the second
/// thing in the archive.
fn empty_state() {
    say!("Nothing captured yet.");
    say!();

    let mechanism = capture_store::hotkey::select(&capture_store::hotkey::Host);
    if mechanism == capture_store::hotkey::Mechanism::None {
        say!("From a terminal — type the text, then press Ctrl-B:");
        say!("  eval \"$(capture shell-init zsh)\"     (or bash, in ~/.bashrc)");
        say!();
        say!("Or directly:");
        say!("  capture + your first note");
        return;
    }

    say!(
        "Press {} anywhere, type, and press Enter:",
        capture_store::hotkey::DEFAULT_CHORD.human()
    );
    say!("  capture-overlay --register     sets the chord up");
    say!("  capture-overlay --run          keeps the window ready");
    say!();
    say!("Or from a terminal:");
    say!("  capture + your first note");
}

/// One listing line: the name, then the text, then the state.
fn row(n: &NoteRow, width: usize, now: u64) -> String {
    let mut marks = Vec::new();
    if n.pinned {
        marks.push("pinned".to_string());
    }
    if let Some(at) = n.redacted_at {
        marks.push(format!("redacted {}", date_of(at, n.tz.as_deref())));
    }
    // In force, not merely present. A note whose mute has ended — or that was
    // unmuted — was still marked `[muted]` in every listing, which said the
    // reversal had not worked while the note sat there in the results.
    if n.is_muted(now) {
        marks.push("muted".to_string());
    }
    if n.divergences > 0 {
        marks.push(format!("{} versions", n.divergences + 1));
    }
    if !n.tags.is_empty() {
        marks.push(
            n.tags
                .iter()
                .map(|t| format!("#{t}"))
                .collect::<Vec<_>>()
                .join(" "),
        );
    }

    let state = if marks.is_empty() {
        String::new()
    } else {
        format!("  [{}]", marks.join(", "))
    };

    // A note with no text is still a note. Saying so beats an empty line,
    // which reads as a rendering bug rather than as a fact about the note.
    let body = match &n.text {
        Some(t) => first_line(t),
        None if n.redacted_at.is_some() => "(content destroyed)".to_string(),
        // Withheld, not yet destroyed. Saying "destroyed" here would be a lie
        // the user could act on — the redaction is still cancellable, and this
        // is the line that has to make that difference visible.
        None if n.redaction_queued => "(content withheld — queued for destruction)".to_string(),
        None => "(no readable text)".to_string(),
    };

    format!("{:<width$}  {}{}", n.slug, body, state)
}

// ----------------------------------------------------------------- show ----

/// One note in full.
pub fn show(paths: &StorePaths, name: &str, json: bool) -> Result<()> {
    let store = open(paths)?;
    let n = find(&store, name)?;
    // A plain note is the one kind with nothing to say — `Promoted::Note`
    // renders to an empty descriptor — and it gets no promotion rather than a
    // label with nothing after it, for the reason at the top of this module.
    let promotion = store.promotion_of(&n.tgt)?.filter(|p| p.kind != "note");
    let links = store.links_from(&n.tgt)?;
    let backlinks = store.links_to(&n.tgt)?;
    let unclear_hour = capture_store::promote::asks_hour(n.text.as_deref(), promotion.as_ref());
    let unclear_day = store.unclear_day_of(&n.tgt)?;
    // A page's checklist lines, numbered as `capture tick-at` takes them.
    let items = match &promotion {
        Some(p) if p.kind == "page" => store.items_of(&n.tgt)?,
        _ => Vec::new(),
    };
    answer(
        &Page {
            items,
            note: n,
            promotion,
            unclear_hour,
            unclear_day,
            links,
            backlinks,
            now: now_ms() as u64,
        },
        json,
    );
    Ok(())
}

/// "Thu 15 Oct" for a stored date, or the date as stored if it is not one.
fn labelled_day(d: &str) -> String {
    capture_time::FloatingDate::parse(d)
        .map(capture_store::calendar::day_label)
        .unwrap_or_else(|| d.to_string())
}

/// One note in full: its fields, what it was recognised as, and its links.
struct Page {
    note: NoteRow,
    promotion: Option<projection::PromotionRow>,
    /// An hour written without am or pm, which the promotion left untimed.
    unclear_hour: Option<u32>,
    /// Two days the text could mean, which is why nothing was promoted.
    unclear_day: Option<[String; 2]>,
    /// A page's checklist lines.
    items: Vec<projection::ListEntry>,
    links: Vec<LinkRow>,
    backlinks: Vec<Backlink>,
    now: u64,
}

impl View for Page {
    fn json(&self) -> Json {
        let Page {
            note: n,
            promotion,
            unclear_hour,
            unclear_day,
            items,
            links,
            backlinks,
            now,
        } = self;
        // What it was recognised as was printed and never carried here, so a
        // script could not ask a note whether it is a task without reading
        // `tasks` and looking for it.
        let promotion = match promotion {
            None => Json::Null,
            Some(projection::PromotionRow {
                kind,
                as_: _, // the decoded fields; `descriptor` is their one rendering
                descriptor,
                pinned,
                grammar: _,    // which recognizer, not what it said
                confidence: _, // likewise
            }) => obj([
                ("kind", kind.into()),
                ("descriptor", descriptor.into()),
                ("authored", (*pinned).into()),
            ]),
        };
        note(n, *now).and([
            ("promotion", promotion),
            ("unclear_hour", (*unclear_hour).into()),
            (
                "unclear_day",
                match unclear_day {
                    Some(days) => arr(days.iter().map(|d| Json::from(d.as_str()))),
                    None => Json::Null,
                },
            ),
            (
                "checklist",
                arr(items.iter().map(|e| {
                    obj([
                        ("ord", e.ord.into()),
                        ("item", e.text.as_str().into()),
                        ("done", e.done.into()),
                    ])
                })),
            ),
            ("links", arr(links.iter().map(link_json))),
            ("backlinks", arr(backlinks.iter().map(backlink_json))),
        ])
    }

    fn prose(&self) {
        let Page {
            note: n,
            promotion,
            unclear_hour,
            unclear_day,
            items,
            links,
            backlinks,
            ..
        } = self;
        say!("{}", n.slug);
        say!("{}", "-".repeat(n.slug.chars().count()));
        if let Some(a) = &n.alias {
            say!("also {a}");
        }

        match &n.text {
            Some(t) => say!("{t}"),
            None if n.redacted_at.is_some() => say!("(content destroyed)"),
            None if n.redaction_queued => {
                say!("(content withheld — queued for destruction)");
                say!();
                say!("It is still recoverable: capture redact --cancel <id>");
                say!("Listed by: capture status");
            }
            None => say!("(no readable text)"),
        }

        say!();

        // What it was recognised as, first, because it is the most
        // consequential thing about a note and this command said none of it. A
        // task never said it was due, a series never said its rule, and
        // `carries-forward` — which only a person can set (§3.6) — had no
        // surface at all to confirm it took.
        //
        // The descriptor is taken from the projection rather than re-rendered
        // here: `promotion_of` renders it once when the fields are written, and
        // a second rendering would be a second version of the truth. The
        // desktop's note page reads the same column, so the two surfaces agree
        // by construction rather than by both being kept up to date.
        if let Some(p) = promotion {
            // A task's descriptor already opens with its own kind, and the
            // other two do not. Printing the label anyway would read "task
            // task · due …", so the repeat comes off here rather than being
            // pre-empted in the renderer, which has surfaces of its own that
            // want the full string. "done" survives this, because that is a
            // state and not the kind.
            let rest = p
                .descriptor
                .strip_prefix(&p.kind)
                .map(|r| r.strip_prefix(" · ").unwrap_or(r))
                .unwrap_or(&p.descriptor);
            // Yours, and how to take it back: a correction outlives every
            // edit, so a mistaken one has to be undone by name (2026-10-13).
            let authored = if p.kind == "page" {
                format!(
                    "  (never read for dates; capture unpage {} to undo)",
                    n.slug
                )
            } else if p.pinned {
                format!("  (yours; capture reread {} to undo)", n.slug)
            } else {
                String::new()
            };
            if rest.is_empty() {
                say!("{}{}", p.kind, authored);
            } else {
                say!("{:<10}{}{}", p.kind, rest, authored);
            }
        }

        // Asked, right under what it was read as, because that line is
        // missing its time and this is why.
        if let Some(h) = unclear_hour {
            say!("time      {h}am or {h}pm?  capture hour {} am|pm", n.slug);
        }
        // Where the promotion would be, because this is why there is none.
        if let Some([a, b]) = unclear_day {
            say!(
                "day       1) {}  or  2) {}?  capture day {} 1|2",
                labelled_day(a),
                labelled_day(b),
                n.slug
            );
        }

        // A page's checklist, numbered for `capture tick-at`.
        if !items.is_empty() {
            say!();
            for e in items.iter() {
                let n_ = e.ord.map(|o| o + 1).unwrap_or(0);
                say!("  {n_:>2}. [{}] {}", if e.done { "x" } else { " " }, e.text);
            }
            say!("  tick one: capture tick-at {} <n>", n.slug);
            say!();
        }

        say!("captured  {}", stamp(n.created.0, n.tz.as_deref()));
        if n.updated != n.created {
            say!("updated   {}", stamp(n.updated.0, n.tz.as_deref()));
        }
        if !n.tags.is_empty() {
            say!("tags      {}", n.tags.join(" "));
        }
        if n.pinned {
            say!("pinned    yes");
        }
        if let Some(until) = n.muted_until {
            say!(
                "muted     {}",
                if until == 0 {
                    "indefinitely".to_string()
                } else {
                    format!("until {}", stamp(until, n.tz.as_deref()))
                }
            );
        }
        if let Some(at) = n.redacted_at {
            say!("redacted  {}", stamp(at, n.tz.as_deref()));
        }
        if n.divergences > 0 {
            // Reported, never prompted. A divergence is information about the
            // note, not a task the reader has to clear before continuing.
            say!(
                "versions  {} independent, none superseded",
                n.divergences + 1
            );
        }

        if !links.is_empty() || !backlinks.is_empty() {
            say!();
            print_links(links, backlinks);
        }
    }
}

// ---------------------------------------------------------------- links ----

/// What a note points at, and what points at it.
pub fn links(paths: &StorePaths, name: &str, json: bool) -> Result<()> {
    let store = open(paths)?;
    let n = find(&store, name)?;
    answer(
        &Links {
            links: store.links_from(&n.tgt)?,
            backlinks: store.links_to(&n.tgt)?,
            slug: n.slug,
        },
        json,
    );
    Ok(())
}

struct Links {
    slug: String,
    links: Vec<LinkRow>,
    backlinks: Vec<Backlink>,
}

impl View for Links {
    fn json(&self) -> Json {
        let Links {
            slug,
            links,
            backlinks,
        } = self;
        obj([
            ("note", slug.into()),
            ("links", arr(links.iter().map(link_json))),
            ("backlinks", arr(backlinks.iter().map(backlink_json))),
        ])
    }

    fn prose(&self) {
        let Links {
            slug,
            links,
            backlinks,
        } = self;
        if links.is_empty() && backlinks.is_empty() {
            say!("{slug} has no links in either direction.");
            return;
        }
        print_links(links, backlinks);
    }
}

fn print_links(out: &[LinkRow], back: &[Backlink]) {
    let width = out
        .iter()
        .map(|l| l.name.chars().count())
        .chain(back.iter().map(|b| b.slug.chars().count()))
        .max()
        .unwrap_or(20)
        .min(44);

    if !out.is_empty() {
        say!("links to");
        for l in out {
            if l.is_broken() {
                // Both names, always. A broken link that cannot say what it was
                // reaching for is not much better than one that vanished.
                say!(
                    "  {:<width$}  broken — was {}",
                    l.name,
                    l.target_slug.as_deref().unwrap_or("a note that is gone")
                );
            } else if l.resolved.is_none() {
                say!("  {:<width$}  never resolved", l.name);
            } else {
                say!("  {}", l.name);
            }
        }
    }
    if !back.is_empty() {
        if !out.is_empty() {
            say!();
        }
        say!("linked from");
        for b in back {
            say!("  {:<width$}  as {}", b.slug, b.name);
        }
    }
}

// --------------------------------------------------------------- search ----

/// Notes matching a query, most relevant first.
pub fn search(paths: &StorePaths, query: &str, json: bool) -> Result<()> {
    let store = open(paths)?;
    // A lapsed mute suppresses nothing, so "now" decides rather than the row.
    let now = now_ms() as u64;
    let hits = store.search(query, now)?;

    let notes: Vec<NoteRow> = hits
        .iter()
        .filter_map(|t| store.note(t).ok().flatten())
        .collect();

    answer(
        &Search {
            query: query.to_string(),
            notes,
            now,
        },
        json,
    );
    Ok(())
}

struct Search {
    query: String,
    notes: Vec<NoteRow>,
    now: u64,
}

impl View for Search {
    fn json(&self) -> Json {
        // The query is the question, not part of the answer.
        let Search {
            query: _,
            notes,
            now,
        } = self;
        arr(notes.iter().map(|n| note(n, *now)))
    }

    fn prose(&self) {
        let Search { query, notes, now } = self;
        if notes.is_empty() {
            say!("Nothing matches {query}.");
            return;
        }
        let width = name_width(notes);
        for n in notes {
            say!("{}", row(n, width, *now));
        }
    }
}

// ----------------------------------------------------------------- time ----

/// The day a note was captured, in the zone it was captured in.
fn day_of(n: &NoteRow) -> String {
    date_of(n.created.0, n.tz.as_deref())
}

fn zone(tz: Option<&str>) -> jiff::tz::TimeZone {
    tz.and_then(|t| jiff::tz::TimeZone::get(t).ok())
        .unwrap_or(jiff::tz::TimeZone::UTC)
}

fn date_of(ms: u64, tz: Option<&str>) -> String {
    let Ok(ts) = jiff::Timestamp::from_millisecond(ms as i64) else {
        return "unknown date".to_string();
    };
    ts.to_zoned(zone(tz)).strftime("%Y-%m-%d %a").to_string()
}

pub(crate) fn stamp(ms: u64, tz: Option<&str>) -> String {
    let Ok(ts) = jiff::Timestamp::from_millisecond(ms as i64) else {
        return "unknown time".to_string();
    };
    ts.to_zoned(zone(tz))
        .strftime("%Y-%m-%d %H:%M %Z")
        .to_string()
}

// ----------------------------------------------------------------- text ----

/// The first line, shortened, for a list.
///
/// Only ever used for lists. `show` prints the text as stored, because a note
/// that can only be read in summary is a note whose content has effectively
/// been edited by the viewer.
fn first_line(text: &str) -> String {
    let line = text.lines().next().unwrap_or("");
    let mut out: String = line.chars().take(60).collect();
    if line.chars().count() > 60 {
        out.push('…');
    }
    out
}

// ----------------------------------------------------------------- json ----

/// A note's own fields, wherever a note is rendered.
///
/// No `..`: a field added to `NoteRow` does not compile here until it is
/// either rendered or set aside with a reason.
fn note(n: &NoteRow, now: u64) -> Json {
    let NoteRow {
        tgt: _, // the name below is how every command refers to a note
        slug,
        alias,
        text,
        created,
        updated,
        tz,
        pinned,
        muted_until,
        redacted_at,
        redaction_queued,
        credential,
        tags,
        parent: _, // no listing or page draws it yet
        divergences,
    } = n;
    // `slug` first: `tools/converge.sh` reads it as the first field.
    obj([
        ("slug", slug.into()),
        ("alias", alias.into()),
        ("text", text.into()),
        ("captured_ms", created.0.into()),
        ("updated_ms", updated.0.into()),
        ("tz", tz.into()),
        ("tags", arr(tags)),
        ("pinned", (*pinned).into()),
        // In force, not merely present. This was `muted_until.is_some()`, so a
        // mute that had ended was reported for ever — the mistake the unmute
        // change took out of ten SQL predicates, still here in the one place a
        // script would look.
        ("muted", n.is_muted(now).into()),
        // `0` is indefinite, as the page's "indefinitely" says.
        ("muted_until", (*muted_until).into()),
        ("redacted", n.is_redacted().into()),
        ("redacted_at", (*redacted_at).into()),
        // Withheld and still cancellable, as against destroyed. The prose has
        // always said which; without this a script saw a note with no text.
        ("redaction_queued", (*redaction_queued).into()),
        ("credential", (*credential).into()),
        ("divergences", divergences.into()),
    ])
}

fn link_json(l: &LinkRow) -> Json {
    let LinkRow {
        name,
        // What `broken` and `resolved` below are computed from.
        resolved: _,
        now: _,
        target_slug,
    } = l;
    obj([
        ("name", name.into()),
        ("resolved", target_slug.into()),
        ("broken", l.is_broken().into()),
    ])
}

fn backlink_json(b: &Backlink) -> Json {
    let Backlink { tgt: _, slug, name } = b;
    obj([("slug", slug.into()), ("as", name.into())])
}

/// What has come due, and what it will do about it.
///
/// Reading, not firing. The whole point of having this separately from the
/// thing that delivers is that "why did nothing happen" has an answer which
/// does not require waiting for the thing to happen again.
pub fn due(paths: &StorePaths, json: bool) -> Result<()> {
    use capture_store::notifier::{Delivery, deliver_late};

    // Read what has not been read, and *say so if that fails*.
    //
    // §3.1 puts parsing after the commit so the capture gesture never waits
    // for the recognizer — which means a capture is not promoted until
    // something reads it, and a read does not. So a fresh
    // `dentist thursday 2pm` was not yet an event, was therefore not yet
    // alertable, and this command reported nothing due.
    //
    // That is the exact failure this command exists to diagnose, so reporting
    // it would have been the tool participating in the bug. The firing path
    // does not need this — it runs inside the due-work drain, which is itself
    // on the far side of a session open — but a person asking "why did nothing
    // happen" is asking before any of that.
    //
    // This used to be `let swept = crate::read::session(…); drop(swept)`, which threw
    // the `Result` away. So a store somebody else was writing gave no sweep, no
    // error and an answer from stale state — the one outcome this command must
    // never produce, since "nothing is due" is exactly what the person is here
    // to disbelieve. §8.8's argument, in the command it was written for.
    let settled = capture_store::promote::settle(paths);

    let store = open(paths)?;
    let now = now_ms();
    let zone = local_zone();

    // A week ahead, not just what has already arrived.
    //
    // The firing path asks `due_by(now)` — what has come due. This one is the
    // answer to "why did nothing happen", and somebody asking that needs to
    // see the thing that has *not* fired yet in order to tell "it is not
    // scheduled" from "it is scheduled and the time has not come". Showing
    // only the overdue would answer the question with silence, which is what
    // they were already looking at.
    const AHEAD_MS: i64 = 7 * 24 * 60 * 60 * 1000;
    let ops = capture_store::ops::Ops::open_read(&paths.ops)?;
    let lead = capture_store::notifier::lead_ms(&ops)?;
    let found = store.announce_by(now + AHEAD_MS, now, &zone, lead)?;

    let hour = hour_of(now, &zone);

    let mut rows = Vec::new();
    for d in found {
        // Said rather than implied. A moment already delivered still shows,
        // because "it fired and you missed it" and "it never fired" are
        // different answers to the same question and only one of them is a
        // bug.
        let state = if ops.alerted(&d.tgt.0, d.occ, d.at)? {
            Will::Delivered
        } else if d.at > now {
            Will::Wait
        } else {
            match deliver_late(hour, d.timed, false, now.saturating_sub(d.at)) {
                Delivery::Now => Will::Now,
                Delivery::At(h) => Will::Hold(h),
                // Said plainly rather than shown as a pending delivery that
                // will never happen. It is still outstanding and still listed;
                // what it will not do is interrupt anybody about it.
                Delivery::Stale => Will::Stale,
            }
        };
        rows.push(Coming {
            slug: store.slug_of(&d.tgt)?.unwrap_or_default(),
            d,
            state,
        });
    }

    answer(
        &DueList {
            rows,
            now,
            zone: capture_time::Zone::new(&zone).unwrap_or_else(utc),
            unread: unread(&store),
            unsettled: settled.err().map(|e| e.to_string()),
        },
        json,
    );
    Ok(())
}

/// Everything due within the week, and what will happen about each.
struct DueList {
    rows: Vec<Coming>,
    now: i64,
    zone: capture_time::Zone,
    unread: usize,
    /// Why the captures could not be read first, if they could not.
    unsettled: Option<String>,
}

struct Coming {
    slug: String,
    d: projection::Due,
    state: Will,
}

/// What delivery will do about one moment.
enum Will {
    Delivered,
    Wait,
    Now,
    /// Held to this hour, because it would otherwise land in quiet hours.
    Hold(u32),
    Stale,
}

impl View for DueList {
    fn json(&self) -> Json {
        let DueList {
            rows,
            now: _,  // already decided each row's state
            zone: _, // `at` is an instant; the zone only draws it
            unread,
            unsettled,
        } = self;
        let items = rows.iter().map(|Coming { slug, d, state }| {
            let projection::Due {
                tgt: _, // named by `slug`
                at,
                occ,
                text,
                timed,
                ahead,
            } = d;
            let (state, held_to) = match state {
                Will::Delivered => ("delivered", None),
                Will::Wait => ("waiting", None),
                Will::Now => ("now", None),
                Will::Hold(h) => ("held", Some(*h)),
                Will::Stale => ("stale", None),
            };
            obj([
                ("slug", slug.into()),
                ("at", at.into()),
                ("occ", (*occ).into()),
                ("timed", (*timed).into()),
                // A warning that far ahead of the moment; zero for the moment.
                ("ahead_ms", (*ahead).into()),
                ("text", text.into()),
                // What it will do. The whole of what this command is for, and
                // the JSON listed the moments without it.
                ("state", state.into()),
                ("held_to", held_to.into()),
            ])
        });
        // An object, so the two admissions the prose makes have somewhere to
        // go: a list read over unread captures, or one that could not read
        // them at all, is not the same answer as the list alone.
        obj([
            ("due", arr(items)),
            ("unread", unread.into()),
            ("unsettled", unsettled.into()),
        ])
    }

    fn prose(&self) {
        let DueList {
            rows,
            now,
            zone,
            unread,
            unsettled,
        } = self;
        if rows.is_empty() {
            say!("Nothing due.");
            say!();
            // Before the explanation, because it changes what the explanation
            // means. "Nothing due" over captures nobody has read is not an
            // answer about scheduling at all.
            say_if_unsettled(*unread, unsettled.as_deref());
            say!("A capture speaks when it names a time — \"dentist thursday 2pm\".");
            say!("One with no time never becomes overdue.");
            return;
        }

        for Coming { d, state, .. } in rows {
            let state = match state {
                Will::Delivered => "delivered".to_string(),
                Will::Wait => "waiting".to_string(),
                Will::Now => "now".to_string(),
                Will::Hold(h) => format!("held to {h:02}:00"),
                Will::Stale => "too late to say".to_string(),
            };
            let when = capture_time::local_of(d.at, *zone).format("%a %H:%M");
            let text = if d.ahead > 0 {
                capture_store::notifier::warning(&d.text, d.ahead)
            } else {
                d.text.clone()
            };
            say!("  {:<13} {:<10} {}", state, when.to_string(), text);
        }

        let overdue = rows.iter().filter(|r| r.d.at <= *now).count();
        say!();
        match overdue {
            0 => say!("{} coming, none due yet.", rows.len()),
            n if n == rows.len() => say!("{n} due."),
            n => say!("{n} due, {} more coming.", rows.len() - n),
        }
        say_if_unsettled(*unread, unsettled.as_deref());
    }
}

/// What this command could not read, and therefore cannot speak for.
///
/// Two different admissions and they are not interchangeable. If the settle
/// *failed* — somebody else holds the writer — then nothing was read and the
/// list below may be missing entries for a reason that has nothing to do with
/// scheduling; say who has the store. If it succeeded, a non-zero count is the
/// ordinary staleness line.
fn say_if_unsettled(unread: usize, unsettled: Option<&str>) {
    if let Some(e) = unsettled {
        say!();
        say!("Captures could not be read first: {e}");
        say!("So this may be missing something. Try again once that finishes.");
        return;
    }
    say_if_behind(unread);
}

/// The local hour, for the delivery decision.
fn hour_of(now: i64, zone: &str) -> u32 {
    let z = capture_time::Zone::new(zone).unwrap_or_else(utc);
    capture_time::civil_clock(now, z).0
}

/// A list: its items in written order, then the tasks carrying its tag.
///
/// §3.7 makes a collection a saved query rather than a folder, and this prints
/// the answer to one. There is no list to create and none to maintain — a list
/// exists for every tag the moment somebody writes one.
pub fn list(paths: &StorePaths, tag: &str, json: bool) -> Result<()> {
    let store = open(paths)?;
    answer(
        &List {
            tag: tag.to_string(),
            rows: store.list(tag, now_ms().max(0) as u64)?,
        },
        json,
    );
    Ok(())
}

struct List {
    tag: String,
    rows: Vec<projection::ListEntry>,
}

impl View for List {
    fn json(&self) -> Json {
        // The tag is the question.
        let List { tag: _, rows } = self;
        arr(rows.iter().map(|e| {
            let projection::ListEntry {
                tgt: _, // the line's source, which no surface names
                ord,
                text,
                done,
                task: _, // an implementation of `done`, not a fact of its own
            } = e;
            obj([
                ("item", text.into()),
                ("done", (*done).into()),
                ("ord", (*ord).into()),
            ])
        }))
    }

    fn prose(&self) {
        let List { tag, rows } = self;
        if rows.is_empty() {
            // Said rather than printed as an empty result, because the two
            // mean different things: nothing carries this tag, as against a
            // list that has been emptied.
            // Not "nothing carries this tag", which would be a claim about the
            // tag rather than about the list. A note can carry `#{tag}` and
            // have no place on a list: the tag page holds everything, and a
            // list holds the half somebody can tick off.
            say!("Nothing on the #{tag} list.");
            say!();
            say!("Start one by capturing:  #{tag}: first thing, second thing");
            say!("Everything carrying the tag:  capture mentions #{tag}");
            return;
        }

        for e in rows {
            // A box rather than a colour, so it survives a pipe and a terminal
            // that has opinions.
            say!("  [{}] {}", if e.done { "x" } else { " " }, e.text);
        }

        let left = rows.iter().filter(|e| !e.done).count();
        say!();
        match left {
            0 => say!("All {} done.", rows.len()),
            n => say!("{n} of {} left.", rows.len()),
        }
    }
}

// -------------------------------------------------------------------- cal ---

/// A month or a week, and what happens on each day.
///
/// The store's calendar read, the same one the phone's grid and the desktop's
/// draw from, so the three cannot disagree about a day.
pub fn cal(paths: &StorePaths, span: &crate::args::CalSpan, json: bool) -> Result<()> {
    use crate::args::CalSpan;
    use capture_store::calendar::Shape;
    let store = open(paths)?;
    let zone = capture_time::Zone::new(&local_zone()).unwrap_or_else(utc);
    let now = now_ms();
    let today = capture_time::civil_date(now, zone);

    let (day, shape) = match span {
        CalSpan::Month(m) => (
            match m {
                None => today,
                Some(m) => capture_time::FloatingDate::parse(&format!("{m}-01"))
                    .ok_or_else(|| Error::Usage(format!("{m:?} is not a month; use YYYY-MM")))?,
            },
            Shape::Month,
        ),
        CalSpan::Week(d) => (
            match d {
                None => today,
                Some(d) => capture_time::FloatingDate::parse(d)
                    .ok_or_else(|| Error::Usage(format!("'{d}' is not a date; use YYYY-MM-DD")))?,
            },
            Shape::Week,
        ),
    };
    // The month's own days only; the grid pads to whole weeks by itself.
    let span = capture_store::calendar::span(day, shape, zone)?;
    let (from, to, month) = (span.first, span.last, shape == Shape::Month);

    let days = capture_store::calendar::days(&store, from, to, zone, now)?;
    answer(
        &Cal {
            days,
            month,
            today,
            zone,
            unread: unread(&store),
        },
        json,
    );
    Ok(())
}

use capture_store::calendar::{DAYS, MONTHS, weekday};

struct Cal {
    days: Vec<capture_store::calendar::Day>,
    /// A whole month, which gets a grid above the list.
    month: bool,
    today: capture_time::FloatingDate,
    zone: capture_time::Zone,
    unread: usize,
}

impl Cal {
    /// "Mon 12 Oct 14:00", for saying where a moved occurrence came from.
    fn when(&self, ms: i64) -> String {
        let d = capture_time::civil_date(ms, self.zone);
        let (h, m) = capture_time::civil_clock(ms, self.zone);
        format!(
            "{} {} {} {h:02}:{m:02}",
            DAYS[weekday(d) as usize],
            d.day,
            &MONTHS[d.month as usize - 1][..3]
        )
    }

    fn clock(&self, ms: i64) -> String {
        let (h, m) = capture_time::civil_clock(ms, self.zone);
        format!("{h:02}:{m:02}")
    }
}

impl View for Cal {
    fn json(&self) -> Json {
        use capture_store::calendar::{Item, ItemKind};
        let Cal {
            days,
            month: _, // a rendering choice; the days say which they are
            today,
            zone: _, // the instants are absolute
            unread,
        } = self;
        let item = |i: &Item| {
            let Item {
                tgt: _, // the slug names it
                slug,
                text,
                kind,
                series,
                occ,
                start_ms,
                end_ms,
                moved_from,
                skipped,
                done,
                title,
                shift,
                unclear_hour,
                unclear_day,
                column: _, // layout for an hour axis; nothing here draws one
                columns: _,
            } = i;
            obj([
                ("slug", slug.into()),
                ("text", text.into()),
                (
                    "kind",
                    match kind {
                        ItemKind::AllDay => "all_day",
                        ItemKind::Timed => "timed",
                        ItemKind::Due => "due",
                    }
                    .into(),
                ),
                ("series", (*series).into()),
                ("occ", (*occ).into()),
                ("start_ms", start_ms.into()),
                ("end_ms", (*end_ms).into()),
                ("moved_from", (*moved_from).into()),
                ("skipped", (*skipped).into()),
                ("done", (*done).into()),
                ("title", title.into()),
                ("shift", shift.into()),
                ("unclear_hour", (*unclear_hour).into()),
                (
                    "unclear_day",
                    match unclear_day {
                        Some(days) => arr(days.iter().map(|d| Json::from(d.to_string()))),
                        None => Json::Null,
                    },
                ),
            ])
        };
        obj([
            ("today", today.to_string().into()),
            (
                "days",
                arr(days.iter().map(|d| {
                    obj([
                        ("date", d.date.to_string().into()),
                        ("busy", d.busy().into()),
                        ("items", arr(d.items.iter().map(item))),
                    ])
                })),
            ),
            ("unread", unread.into()),
        ])
    }

    fn prose(&self) {
        use capture_store::calendar::ItemKind;
        let Some(first) = self.days.first() else {
            return;
        };

        if self.month {
            // A grid first, as `cal` draws one: a * beside a day with
            // something on it, brackets around today.
            say!(
                "{} {}",
                MONTHS[first.date.month as usize - 1],
                first.date.year
            );
            say!("{}", DAYS.map(|d| format!(" {:<3}", &d[..2])).join(""));
            let mut line = "    ".repeat(weekday(first.date) as usize);
            for d in &self.days {
                let mark = if d.busy() { '*' } else { ' ' };
                let cell = if d.date == self.today {
                    format!("[{:>2}]", d.date.day)
                } else {
                    format!(" {:>2}{mark}", d.date.day)
                };
                line.push_str(&cell);
                if weekday(d.date) == 6 {
                    say!("{}", line.trim_end());
                    line.clear();
                }
            }
            if !line.trim().is_empty() {
                say!("{}", line.trim_end());
            }
            say!();
        }

        let mut any = false;
        for d in &self.days {
            if d.items.is_empty() {
                continue;
            }
            any = true;
            say!(
                "{} {} {}",
                DAYS[weekday(d.date) as usize],
                d.date.day,
                &MONTHS[d.date.month as usize - 1][..3]
            );
            for i in &d.items {
                let at = match i.kind {
                    ItemKind::AllDay => "all day".to_string(),
                    ItemKind::Due => "due".to_string(),
                    ItemKind::Timed => match i.end_ms {
                        Some(e) => format!("{}–{}", self.clock(i.start_ms), self.clock(e)),
                        None => self.clock(i.start_ms),
                    },
                };
                let what = i
                    .title
                    .clone()
                    .or_else(|| i.text.clone())
                    .unwrap_or_default();
                let mut notes = Vec::new();
                if let Some(from) = i.moved_from {
                    notes.push(format!("moved from {}", self.when(from)));
                }
                if i.skipped {
                    notes.push(format!("skipped — capture unskip {} {}", i.slug, d.date));
                }
                if i.done {
                    notes.push("done".into());
                }
                if let Some(h) = i.unclear_hour {
                    notes.push(format!("{h}am or {h}pm? capture hour {} am|pm", i.slug));
                }
                if let Some([a, b]) = i.unclear_day {
                    let label = capture_store::calendar::day_label;
                    notes.push(format!(
                        "{} or {}? capture day {} 1|2",
                        label(a),
                        label(b),
                        i.slug
                    ));
                }
                let notes = if notes.is_empty() {
                    String::new()
                } else {
                    format!("  ({})", notes.join("; "))
                };
                say!("  {at:<11}  {what}  [{}]{notes}", i.slug);
            }
        }
        if !any {
            say!("Nothing on these days.");
        }
        say_if_behind(self.unread);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_long_line_is_shortened_only_in_lists() {
        let long = "x".repeat(200);
        assert!(first_line(&long).chars().count() <= 61);
        assert!(first_line(&long).ends_with('…'));
    }

    #[test]
    fn only_the_first_line_appears_in_a_list() {
        assert_eq!(first_line("first\nsecond\nthird"), "first");
    }

    #[test]
    fn short_text_is_left_alone() {
        assert_eq!(first_line("brief"), "brief");
    }

    #[test]
    fn a_day_uses_the_capture_zone_not_the_readers() {
        // 2026-01-01 05:30 UTC is still New Year's Eve in Chicago. A timeline
        // grouped in the reader's zone would move the note to a different day
        // depending on where it was read, which is exactly what breaks recall
        // for notes made while travelling.
        let ms = 1_767_245_400_000u64;
        assert!(date_of(ms, Some("UTC")).starts_with("2026-01-01"));
        assert!(date_of(ms, Some("America/Chicago")).starts_with("2025-12-31"));
    }

    #[test]
    fn an_unknown_zone_falls_back_rather_than_failing() {
        // A note must remain readable even if its zone is one this machine has
        // never heard of.
        assert!(date_of(1_767_245_400_000, Some("Mars/Olympus")).starts_with("2026-01-01"));
        assert!(date_of(1_767_245_400_000, None).starts_with("2026-01-01"));
    }
}
