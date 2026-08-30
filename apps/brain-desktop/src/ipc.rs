//! What the webview may ask for.
//!
//! # The whole boundary is here
//!
//! Everything the interface can do is a function in this file. There is no
//! server, no port, and no way for the page to reach the store except through
//! one of these — which is the point of writing them out rather than exposing a
//! query interface: the surface is enumerable, and adding to it is a deliberate
//! act with a diff.
//!
//! # Reads are read-only, writes take the lock
//!
//! Read commands open the projection read-only and hold nothing, so the window
//! keeps working while the command line or the overlay is writing. Write
//! commands open a session, which takes the writer lock for the length of the
//! call and releases it — never for the lifetime of the window, or the desktop
//! app being open would stop every other binary from working.
//!
//! # Errors cross as strings
//!
//! The webview cannot do anything useful with a typed error, and the page has
//! no business branching on error kinds — it shows what went wrong. Exit codes
//! matter at a process boundary, and this is not one.

use brain_store::{
    Error, Result, StorePaths,
    ops::Ops,
    projection::{self, NoteRow, Store},
    session::Session,
};
use serde::Serialize;

/// A note, as the interface needs it.
///
/// Deliberately not the storage type. This is a wire format the page depends
/// on, and letting a storage field rename ripple into the interface is how a
/// refactor becomes a broken window.
#[derive(Serialize)]
pub struct Note {
    pub slug: String,
    pub alias: Option<String>,
    pub text: Option<String>,
    pub captured_ms: u64,
    pub updated_ms: u64,
    pub tz: Option<String>,
    pub tags: Vec<String>,
    pub pinned: bool,
    pub muted: bool,
    pub redacted: bool,
    pub redacted_ms: Option<u64>,
    pub divergences: u64,
    /// Which of the four states the row renders as.
    ///
    /// Decided here rather than in the page, so every surface agrees about what
    /// a note *is* — and so a state added later cannot be silently missed by a
    /// template that only knew about four.
    pub state: &'static str,
}

impl From<NoteRow> for Note {
    fn from(n: NoteRow) -> Note {
        let state = if n.redacted_at.is_some() {
            "redacted"
        } else if n.redaction_queued {
            // Ordered and not yet carried out. The content is already withheld
            // — that is not delayed — but it is still cancellable, and a reader
            // told "destroyed" has been told something they can no longer act
            // on when in fact they can.
            "withheld"
        } else if n.text.is_none() {
            // Not the same as redacted. A note whose content has not arrived is
            // a different fact from one whose content was destroyed, and a
            // reader shown the wrong one has been told the wrong story about
            // their own archive.
            "incomplete"
        } else if n.divergences > 0 {
            "diverged"
        } else if n.muted_until.is_some() {
            "muted"
        } else {
            "ordinary"
        };

        Note {
            slug: n.slug,
            alias: n.alias,
            text: n.text,
            captured_ms: n.created.0,
            updated_ms: n.updated.0,
            tz: n.tz,
            tags: n.tags,
            pinned: n.pinned,
            muted: n.muted_until.is_some(),
            redacted: n.redacted_at.is_some(),
            redacted_ms: n.redacted_at,
            divergences: n.divergences,
            state,
        }
    }
}

/// A link, with enough to render it broken.
#[derive(Serialize)]
pub struct Link {
    pub name: String,
    /// What the name meant when it was written, if that note still exists.
    pub target: Option<String>,
    pub broken: bool,
}

/// A note that points here.
#[derive(Serialize)]
pub struct Backlink {
    pub slug: String,
    /// The name it used, which may not be this note's current one.
    pub name: String,
}

/// One run of a note's text, or a link inside it.
///
/// The body is sent pre-split rather than as a string the page scans itself.
/// Two scanners over one syntax is two answers to one question: the body would
/// eventually disagree with the list of links beneath it about what the note
/// links to, and neither would be visibly wrong.
#[derive(Serialize)]
pub struct Piece {
    /// Text as written. For a link this is the name between the brackets.
    pub text: String,
    /// Present only for a link: the note to navigate to, if the name still
    /// denotes one.
    pub target: Option<String>,
    /// A link, rather than a run of text.
    pub link: bool,
    /// A mention — `@sarah`, `^acme` — which navigates to that name's page.
    ///
    /// Carried on the same pieces as links because they are the same thing to a
    /// reader: something in the text you can follow. They are kept apart
    /// underneath because they resolve differently — a link points at one note
    /// and can break, a mention is a query that always has an answer, even if
    /// the answer is one note.
    pub mention: Option<char>,
    /// What became of the name. Three outcomes, not two — the first version had
    /// only "broken or not" and rendered a name matching nothing as an ordinary
    /// followable link, which went nowhere when clicked.
    ///
    /// - `"ok"` — it still denotes what it did; follow it.
    /// - `"moved"` — the name now means something else. §4.3: silently pointing
    ///   an old reference at new content is how a memory system starts lying to
    ///   you, so this renders as broken showing the name as written.
    /// - `"unresolved"` — nothing has that name. Not an error and not broken:
    ///   forward references are a normal way to write, and the note that
    ///   answers one may not exist yet. Shown as written, not followable.
    pub state: &'static str,
}

/// Everything the note page shows.
#[derive(Serialize)]
pub struct Page {
    pub note: Note,
    /// The text, split into what to show and what to make clickable.
    pub body: Vec<Piece>,
    pub links: Vec<Link>,
    pub backlinks: Vec<Backlink>,
    pub versions: u64,
    /// What this note was recognised as, if anything.
    pub promotion: Option<Promotion>,
    /// A reading the recognizer had but did not act on.
    pub suggestion: Option<String>,
}

/// What a note currently is, beside what was captured.
///
/// §3.1.4 keeps the two apart on purpose: the text is the episodic record and
/// the fields are operationally authoritative, and neither is regenerated from
/// the other. Showing both is what makes drift visible instead of silent.
#[derive(Serialize)]
pub struct Promotion {
    /// `note`, `task`, `event` or `series`.
    pub kind: String,
    /// The one-line rendering of the fields.
    pub descriptor: String,
    /// Whether a person assigned it, and therefore whether a reparse may
    /// replace it.
    pub pinned: bool,
    /// The confidence behind it, where a recognizer produced it.
    pub confidence: Option<u32>,
}

/// One occurrence of a series, as a surface draws it.
#[derive(Serialize)]
pub struct Occurrence {
    pub slug: String,
    /// The occurrence's name — its original start.
    pub occ: i64,
    pub start_ms: i64,
    pub all_day: bool,
    pub done: bool,
    /// `gap` or `ambiguous`, when a daylight-saving transition moved it.
    pub shift: Option<String>,
    pub text: Option<String>,
    /// When it ends, where an end was written.
    ///
    /// Absent is the common case and is not a defect: §3.3's "30m default" is
    /// deliberately not invented by the promoter, because a wrong end time is
    /// harder to notice than a missing one. The grid draws these as a mark at
    /// their start rather than as a block, so nothing on screen claims a length
    /// nobody wrote.
    pub end_ms: Option<i64>,
    /// Which column this occupies where occurrences overlap, and how many
    /// columns its cluster needs. Computed here rather than in the page so
    /// every surface packs a day the same way (§4.4).
    pub column: usize,
    pub columns: usize,
    /// A task due on this day rather than something that occurs on it.
    ///
    /// The two are drawn together because §4.4's agenda is "what is due and
    /// what occurs" and somebody looking at a day wants one answer. They are
    /// still distinguished, because completing them is not the same operation:
    /// an occurrence takes `occ_done` against its start, and a task is a
    /// `promote` carrying its state.
    pub due: bool,
    /// Where a due task's date has already passed, the day it was due.
    pub was_due: Option<String>,
}

/// One note offered in a review session.
#[derive(Serialize)]
pub struct ReviewItem {
    pub slug: String,
    pub text: Option<String>,
    /// `forward`, `never-engaged` or `context` — why this one was chosen.
    pub because: &'static str,
    /// A reading the recognizer had and did not act on, offered here as one
    /// line. §3.1.1 keeps it an offer: nothing has been applied to this note.
    pub suggestion: Option<String>,
}

/// A series that has lapsed often enough to be worth one question.
#[derive(Serialize)]
pub struct Nagging {
    pub slug: String,
}

/// A whole session, which is a fixed small number of items and then over.
#[derive(Serialize)]
pub struct ReviewSession {
    pub items: Vec<ReviewItem>,
    /// Asked once, not inferred (§3.6). A parser cannot tell a missed walk from
    /// a missed dose, and guessing wrong builds the backlog this system exists
    /// to not have.
    pub nagging: Vec<Nagging>,
}

/// One thing worth telling the user.
#[derive(Serialize)]
pub struct Condition {
    /// `attention` or `worth-knowing`. Tiered so that a surface full of
    /// undifferentiated warnings does not train the reader to ignore all of
    /// them — most of what is reported here is the system working.
    pub tier: &'static str,
    pub text: String,
    /// Whether the user can dismiss it. True only for conditions this program
    /// cannot verify have been dealt with.
    pub acknowledgeable: bool,
}

/// The health surface.
#[derive(Serialize)]
pub struct Health {
    pub notes: u64,
    pub events: u64,
    pub conditions: Vec<Condition>,
    /// Said out loud rather than left as an empty list: an empty surface is
    /// ambiguous between "nothing is wrong" and "nothing was checked".
    pub healthy: bool,
    /// What this surface did *not* check, named so it cannot be mistaken for a
    /// verification.
    pub unchecked: String,
}

/// A redaction waiting for its window to elapse.
#[derive(Serialize)]
pub struct Pending {
    pub id: String,
    pub slug: String,
    pub execute_after_ms: u64,
}

fn paths() -> Result<StorePaths> {
    StorePaths::resolve()
        .ok_or_else(|| Error::Usage("cannot work out where the store lives".into()))
}

/// Open the projection without taking the writer lock.
fn read_only() -> Result<Store> {
    let p = paths()?;
    let generation = Ops::open_read(&p.ops)?.generation()?;
    Store::open_read(&projection::generation_path(&p.state, generation))
}

fn wire<T>(r: Result<T>) -> std::result::Result<T, String> {
    r.map_err(|e| e.to_string())
}

// ------------------------------------------------------------------ reads ----

#[tauri::command]
pub fn timeline(limit: usize) -> std::result::Result<Vec<Note>, String> {
    wire((|| {
        let s = read_only()?;
        Ok(s.timeline(limit)?.into_iter().map(Note::from).collect())
    })())
}

#[tauri::command]
pub fn note(name: String) -> std::result::Result<Page, String> {
    wire((|| {
        let s = read_only()?;
        let Some(tgt) = s.resolve(&name)? else {
            return Err(Error::NotFound(name.clone()));
        };
        let row = s.note(&tgt)?.ok_or_else(|| Error::NotFound(name.clone()))?;

        let links_now = s.links_from(&tgt)?;
        let links = links_now
            .iter()
            .cloned()
            .map(|l| Link {
                broken: l.is_broken(),
                target: l.target_slug,
                name: l.name,
            })
            .collect();

        let backlinks = s
            .links_to(&tgt)?
            .into_iter()
            .map(|b| Backlink {
                slug: b.slug,
                name: b.name,
            })
            .collect();

        // The body, split once here so the page never has to find a link
        // itself. Zipped against `links` in order, which is what `ord` in that
        // table is for: the Nth link in the text is the Nth row.
        let mut nth = 0;
        let body: Vec<Piece> = row
            .text
            .as_deref()
            .map(brain_core::link::segments)
            .unwrap_or_default()
            .into_iter()
            .flat_map(|p| match p {
                // A run of plain text is split again on the mentions in it, so
                // `@sarah` is followable in the middle of a sentence. Two
                // scanners, but over different syntaxes owned by different
                // crates: links are the fold's, sigils are the recognizer's.
                brain_core::link::Piece::Text(t) => split_mentions(&t),
                brain_core::link::Piece::Link(name) => {
                    let row = links_now.get(nth);
                    nth += 1;
                    // Followed to where the name means *now*. Following the
                    // authoring-time target instead would navigate to content
                    // the name no longer names.
                    let target = row.and_then(|l: &projection::LinkRow| l.target_slug.clone());
                    let state = match row {
                        Some(l) if l.is_broken() => "moved",
                        Some(_) if target.is_some() => "ok",
                        // Either nothing by that name, or a target that is no
                        // longer here. Both are "there is nowhere to go", and
                        // neither is a lie about what the note says.
                        _ => "unresolved",
                    };
                    vec![Piece {
                        text: name,
                        target,
                        link: true,
                        mention: None,
                        state,
                    }]
                }
            })
            .collect();

        let versions = row.divergences + 1;

        let promotion = s.promotion_of(&tgt)?.map(|p| Promotion {
            kind: p.kind,
            descriptor: p.descriptor,
            pinned: p.pinned,
            confidence: p.confidence,
        });

        // A reading the recognizer had and did not act on, offered as one line.
        //
        // Computed here rather than stored, because a suggestion is derived
        // data and not a decision — writing one into the log would make an
        // event out of a guess. Only offered where nothing was promoted: once a
        // note has a type, a competing reading is noise rather than an offer.
        let suggestion = match (&promotion, row.text.as_deref()) {
            (None, Some(text)) => suggestion_for(text, row.tz.as_deref()),
            _ => None,
        };

        Ok(Page {
            note: Note::from(row),
            body,
            links,
            backlinks,
            versions,
            promotion,
            suggestion,
        })
    })())
}

/// Split a run of text so its `@person` and `^project` are followable.
///
/// The sigil stays in the rendered text. It is what somebody typed, it is how
/// they will search for it, and hiding it would make the page a paraphrase of
/// the note rather than the note.
fn split_mentions(text: &str) -> Vec<Piece> {
    let found = brain_parse::mentions(text);
    if found.is_empty() {
        return vec![Piece {
            text: text.to_string(),
            target: None,
            link: false,
            mention: None,
            state: "ok",
        }];
    }

    let mut out = Vec::new();
    let mut rest = text;
    for (sigil, value) in found {
        let whole = format!("{sigil}{value}");
        // Searched for rather than sliced by index, because the tokenizer's
        // spans are its own and this text has already been cut once by the link
        // scanner. A mention that cannot be found again is left as text, which
        // is the safe direction: the reader sees exactly what was written.
        let Some(at) = rest.find(&whole) else {
            continue;
        };
        if at > 0 {
            out.push(Piece {
                text: rest[..at].to_string(),
                target: None,
                link: false,
                mention: None,
                state: "ok",
            });
        }
        out.push(Piece {
            text: value,
            target: None,
            link: false,
            mention: Some(sigil),
            state: "ok",
        });
        rest = &rest[at + whole.len()..];
    }
    if !rest.is_empty() {
        out.push(Piece {
            text: rest.to_string(),
            target: None,
            link: false,
            mention: None,
            state: "ok",
        });
    }
    out
}

/// Every note naming a person or a project.
///
/// §4.3's entity page, which is a filtered timeline and nothing more. There is
/// no record to create: a page exists for every name the moment it is written,
/// which is what makes the layer usable on day one instead of after months of
/// curation.
#[tauri::command]
pub fn mentioning(sigil: String, value: String) -> std::result::Result<Vec<Note>, String> {
    wire((|| {
        let c = sigil
            .chars()
            .next()
            .ok_or_else(|| Error::Usage("a mention needs its sigil".into()))?;
        let s = read_only()?;
        Ok(s.mentioning(c, &value, now_ms())?
            .into_iter()
            .map(Note::from)
            .collect())
    })())
}

/// Every note carrying a tag.
///
/// The same page as a mention, from a different table. A tag is an OR-Set with
/// its own events where a mention is part of the text (§3.7) — a real
/// difference underneath, and none at all to somebody following one.
#[tauri::command]
pub fn tagged(tag: String) -> std::result::Result<Vec<Note>, String> {
    wire((|| {
        let s = read_only()?;
        Ok(s.tagged(&tag, now_ms())?
            .into_iter()
            .map(Note::from)
            .collect())
    })())
}

/// One entry in the wiki's sidebar.
#[derive(Serialize)]
pub struct Entry {
    /// What to show: an alias, or a name without its sigil.
    pub name: String,
    /// For a page, the note to open. Absent for a person, project or tag,
    /// which open a page of their own.
    pub slug: Option<String>,
    /// The page this one sits under, by slug. Only pages nest — a person is
    /// not under a project, and pretending otherwise would invent a
    /// relationship nobody wrote.
    pub parent: Option<String>,
    /// How many notes name it. Absent for a page, which is one note.
    pub count: Option<u32>,
}

/// The sidebar: everything there is to browse.
#[derive(Serialize)]
pub struct Index {
    /// Notes given a name by hand.
    pub pages: Vec<Entry>,
    pub people: Vec<Entry>,
    pub projects: Vec<Entry>,
    pub tags: Vec<Entry>,
}

/// What the wiki can reach.
///
/// Four lists, none of them maintained by anybody. Pages are the notes somebody
/// aliased — the one act that says "this is a thing I will come back to" — and
/// the other three are what the notes happen to say. There is nothing to file
/// into and nothing to prune, which is §3.7's whole argument applied to a
/// sidebar: the moment a list needs tending it stops being tended.
#[tauri::command]
pub fn wiki_index() -> std::result::Result<Index, String> {
    wire((|| {
        let s = read_only()?;

        let pages = s
            .named()?
            .into_iter()
            .map(|n| Entry {
                name: n.alias,
                slug: Some(n.slug),
                parent: n.parent,
                count: None,
            })
            .collect();

        let listed = |sigil: char| -> Result<Vec<Entry>> {
            Ok(s.distinct_mentions(sigil)?
                .into_iter()
                .map(|(name, count)| Entry {
                    name,
                    slug: None,
                    parent: None,
                    count: Some(count),
                })
                .collect())
        };

        Ok(Index {
            pages,
            people: listed('@')?,
            projects: listed('^')?,
            tags: listed('#')?,
        })
    })())
}

/// Put a page under another, or back at the top.
#[tauri::command]
pub fn set_parent(name: String, under: Option<String>) -> std::result::Result<(), String> {
    wire((|| {
        brain_store::mutate::set_parent(&paths()?, &name, under.as_deref())
    })())
}

/// What the recognizer saw, when it was not sure enough to act.
///
/// Deliberately says how sure it was. A suggestion the reader cannot calibrate
/// is one they either always take or always ignore, and §3.1.3 wants it to cost
/// a glance either way.
fn suggestion_for(text: &str, tz: Option<&str>) -> Option<String> {
    let zone = tz
        .and_then(brain_time::Zone::new)
        .unwrap_or_else(|| brain_time::Zone::new("UTC").expect("UTC is always present"));

    let r = brain_parse::recognize(text, now_ms() as i64, zone);
    if r.confidence == 0 || r.confidence >= brain_store::promote::FLOOR {
        return None;
    }

    let what = match &r.shape {
        brain_parse::Shape::Task { due: None } => String::from("a task"),
        brain_parse::Shape::Task { due: Some(d) } => format!("a task due {d}"),
        brain_parse::Shape::Point(brain_parse::When::Date(d)) => format!("something on {d}"),
        brain_parse::Shape::Point(brain_parse::When::Moment(ms)) => {
            let (h, m) = brain_time::civil_clock(*ms, zone);
            format!(
                "something on {} at {h:02}:{m:02}",
                brain_time::civil_date(*ms, zone)
            )
        }
        brain_parse::Shape::Range { from, to } => format!("something from {from} to {to}"),
        brain_parse::Shape::Recurring { .. } => String::from("something recurring"),
        brain_parse::Shape::Nothing => return None,
    };

    Some(format!(
        "This might be {what} — {}% sure, which is under the bar for deciding on its own.",
        r.confidence
    ))
}

/// Occurrences and one-off events on a day.
#[tauri::command]
pub fn agenda(day: String) -> std::result::Result<Vec<Occurrence>, String> {
    wire((|| {
        let s = read_only()?;
        let zone = brain_time::Zone::new("UTC").expect("UTC is always present");
        let date = brain_time::FloatingDate::parse(&day)
            .ok_or_else(|| Error::Usage(format!("'{day}' is not a date")))?;
        let from = brain_time::at_local(date, 0, 0, zone)
            .ok_or_else(|| Error::Usage("that day does not exist".into()))?;
        let to = from + 24 * 60 * 60 * 1000;

        let mut out = Vec::new();

        // All-day entries first, in their own band. They are floating dates and
        // have no position on an hour axis (§4.4).
        for tgt in s.all_day_on(&day)? {
            out.push(Occurrence {
                slug: s.slug_of(&tgt)?.unwrap_or_default(),
                occ: from,
                start_ms: from,
                all_day: true,
                done: false,
                shift: None,
                text: s.text_of(&tgt)?,
                end_ms: None,
                column: 0,
                columns: 1,
                due: false,
                was_due: None,
            });
        }

        for (tgt, o) in s.happening_between(from, to)? {
            // Hidden, not gone: the cancellation is an ordinary event. One that
            // was also moved is drawn, because a move outranks a cancel (§6).
            if o.cancelled && o.start_ms == o.occ {
                continue;
            }
            out.push(Occurrence {
                slug: s.slug_of(&tgt)?.unwrap_or_default(),
                occ: o.occ,
                start_ms: o.start_ms,
                all_day: o.all_day,
                done: o.done,
                shift: o.shift,
                text: o.title.clone().or(s.text_of(&tgt)?),
                end_ms: o.end_ms,
                column: 0,
                columns: 1,
                due: false,
                was_due: None,
            });
        }

        // What is due, beside what occurs. Until this existed the window drew
        // only the second half: a task due today was listed by `brain tasks`
        // and invisible on the day it mattered, which is the same failure shape
        // as an intent that is registered and never fires.
        for (tgt, done) in s.tasks_due_on(&day)? {
            out.push(Occurrence {
                slug: s.slug_of(&tgt)?.unwrap_or_default(),
                occ: from,
                start_ms: from,
                all_day: true,
                done,
                shift: None,
                text: s.text_of(&tgt)?,
                end_ms: None,
                column: 0,
                columns: 1,
                due: true,
                was_due: None,
            });
        }

        // Overdue tasks, on today only. Still open rather than lapsed — §3.6
        // makes a task the one thing here that genuinely carries forward, and
        // saying so once is not a backlog.
        let today = format!("{}", brain_time::civil_date(now_ms() as i64, zone));
        if day == today {
            for tgt in s.tasks_overdue(&today)? {
                let was = s.promotion_of(&tgt)?.and_then(|p| match p.as_ {
                    brain_core::body::Promoted::Task(t) => t.due,
                    _ => None,
                });
                out.push(Occurrence {
                    slug: s.slug_of(&tgt)?.unwrap_or_default(),
                    occ: from,
                    start_ms: from,
                    all_day: true,
                    done: false,
                    shift: None,
                    text: s.text_of(&tgt)?,
                    end_ms: None,
                    column: 0,
                    columns: 1,
                    due: true,
                    was_due: was,
                });
            }
        }

        pack(&mut out);
        Ok(out)
    })())
}

/// How long an occurrence with no written end occupies, for overlap only.
///
/// Not a duration and never drawn as one. Two things at the same time are still
/// two things at the same time, and a zero-width interval overlaps nothing — so
/// packing would put them in one column and the page would draw them on top of
/// each other. Fifteen minutes is enough to make them collide and short enough
/// that it cannot manufacture a collision between meetings an hour apart.
const UNWRITTEN: i64 = 15 * 60 * 1000;

/// Assign each timed occurrence a column, so overlapping ones sit side by side.
///
/// §4.4 asks for column packing and does not say how. This is the ordinary
/// algorithm: occurrences are grouped into clusters that transitively overlap,
/// and within a cluster each takes the first column free at its start time. The
/// cluster is what decides the width — every occurrence in it is drawn at
/// `1/columns`, so a day with one three-way overlap at noon does not squeeze the
/// morning.
///
/// All-day entries and due tasks are left alone: they have no position on an
/// hour axis, which is the whole reason §3.2 keeps them in a band of their own.
fn pack(rows: &mut [Occurrence]) {
    let mut timed: Vec<usize> = (0..rows.len())
        .filter(|&i| !rows[i].all_day && !rows[i].due)
        .collect();

    timed.sort_by_key(|&i| (rows[i].start_ms, -end_of(&rows[i])));

    let mut cluster: Vec<usize> = Vec::new();
    let mut cluster_end = i64::MIN;
    // The end of the last occurrence in each open column.
    let mut columns: Vec<i64> = Vec::new();

    for &i in &timed {
        // A gap closes the cluster: nothing after this point can overlap
        // anything in it, so the width is settled and can be written down.
        if rows[i].start_ms >= cluster_end && !cluster.is_empty() {
            let width = columns.len().max(1);
            for &j in &cluster {
                rows[j].columns = width;
            }
            cluster.clear();
            columns.clear();
            cluster_end = i64::MIN;
        }

        let col = match columns.iter().position(|&end| end <= rows[i].start_ms) {
            Some(c) => c,
            None => {
                columns.push(i64::MIN);
                columns.len() - 1
            }
        };

        let end = end_of(&rows[i]);
        columns[col] = end;
        rows[i].column = col;
        cluster.push(i);
        cluster_end = cluster_end.max(end);
    }

    let width = columns.len().max(1);
    for &j in &cluster {
        rows[j].columns = width;
    }
}

fn end_of(o: &Occurrence) -> i64 {
    o.end_ms.unwrap_or(o.start_ms + UNWRITTEN)
}

/// Mark a task done, or open again.
///
/// Not `set_occurrence`, and the difference is §3.6's: a task has state of its
/// own, where a series has state per instance and nothing accumulates between
/// them. Sending one through the other would either lose the due date or
/// invent an occurrence that no rule produced.
#[tauri::command]
pub fn set_task_done(name: String, done: bool) -> std::result::Result<(), String> {
    wire((|| brain_store::promote::set_done(&paths()?, &name, done))())
}

/// Mark one occurrence complete, or not.
#[tauri::command]
pub fn set_occurrence(name: String, occ: i64, done: bool) -> std::result::Result<(), String> {
    wire((|| {
        let mut s = Session::open(paths()?)?;
        let tgt = s
            .state
            .resolve(&name)?
            .ok_or_else(|| Error::NotFound(name.clone()))?;
        let kind = if done {
            brain_core::event::Kind::OCC_DONE
        } else {
            brain_core::event::Kind::OCC_UNDONE
        };
        s.append(kind, Some(tgt), None, |nonce| {
            let mut b = Vec::new();
            minicbor::encode(&brain_core::body::OccBody { nonce, occ }, &mut b)
                .expect("encoding into Vec is infallible");
            b
        })?;
        Ok(())
    })())
}

/// Names beginning with what has been typed, for completing a link.
///
/// §4.3 specifies `[[` autocomplete, and the reason is that the alternative is
/// worse than no linking at all: a slug ends in ten base32 characters nobody
/// will retype, so writing a link by hand means going and finding the note to
/// copy its name — which is the filing ritual §3.7 exists to avoid.
#[tauri::command]
pub fn names(prefix: String) -> std::result::Result<Vec<String>, String> {
    wire((|| read_only()?.names_like(&prefix, 8))())
}

#[tauri::command]
pub fn search(query: String) -> std::result::Result<Vec<Note>, String> {
    wire((|| {
        let s = read_only()?;
        let now = now_ms();
        let mut out = Vec::new();
        for tgt in s.search(&query, now)? {
            if let Some(n) = s.note(&tgt)? {
                out.push(Note::from(n));
            }
        }
        Ok(out)
    })())
}

/// What was going on when a note was captured.
#[tauri::command]
pub fn ambient(name: String) -> std::result::Result<Vec<(String, String)>, String> {
    wire((|| {
        let s = read_only()?;
        let Some(tgt) = s.resolve(&name)? else {
            return Err(Error::NotFound(name.clone()));
        };
        s.ambient_of(&tgt)
    })())
}

/// Notes sharing an ambient value, for the context strip's click-to-filter.
#[tauri::command]
pub fn filter_by_context(field: String, value: String) -> std::result::Result<Vec<Note>, String> {
    wire((|| {
        let s = read_only()?;
        Ok(s.by_context(&field, &value)?
            .into_iter()
            .map(Note::from)
            .collect())
    })())
}

/// What a reader has already accounted for.
///
/// Held for the lifetime of the window so that a change is reported once
/// rather than every time it is asked about.
#[derive(Default)]
pub struct Watcher(pub std::sync::Mutex<Option<brain_store::watch::Watch>>);

/// Whether anything has moved since the last look.
///
/// The page asks on a short timer. `watch.rs` argues for polling over being
/// told, and the argument is about failure rather than latency: a reader that
/// waits to be notified shows stale data forever if the message is dropped or
/// the writer died between committing and notifying, while a reader that looks
/// for itself is at worst late. Two integer reads, through read-only
/// connections that do not need the writer to be running.
///
/// Two different things move and both matter. New events mean a capture
/// happened — from the overlay, the terminal chord, or another window. A new
/// generation means the projection was rebuilt underneath us, and the file this
/// window has been reading is one nobody writes to any more: same data, frozen.
#[tauri::command]
pub fn changed(watcher: tauri::State<'_, Watcher>) -> std::result::Result<bool, String> {
    wire((|| {
        let p = paths()?;
        let ops = Ops::open_read(&p.ops)?;
        let log = brain_store::log::Log::open_read(&p.events)?;

        let mut guard = watcher.0.lock().unwrap_or_else(|e| e.into_inner());
        match guard.as_mut() {
            Some(w) => Ok(w.poll(&log, &ops)?.any()),
            None => {
                // The first look establishes where things stand. Reporting a
                // change here would refresh the page immediately after it drew.
                *guard = Some(brain_store::watch::Watch::start(&log, &ops)?);
                Ok(false)
            }
        }
    })())
}

#[tauri::command]
pub fn health() -> std::result::Result<Health, String> {
    wire((|| {
        let p = paths()?;
        let s = read_only()?;
        let ops = Ops::open_read(&p.ops)?;
        let log = brain_store::log::Log::open_read(&p.events)?;
        let now = now_ms();

        let mut conditions = Vec::new();

        // Tier one: things wanting a decision. Authentication failures are
        // never folded into a general total — an event that failed to
        // authenticate is a security signal, and burying it alongside version
        // skew is how it goes unnoticed.
        for (reason, n) in ops.rejected_counts()? {
            conditions.push(Condition {
                tier: "attention",
                text: format!("{n} event(s) failed authentication ({reason})"),
                acknowledgeable: false,
            });
        }
        if brain_store::spool::in_use(&p.spool)? {
            conditions.push(Condition {
                tier: "attention",
                text: "captures are being spooled — the store could not be written".into(),
                acknowledgeable: false,
            });
        }
        // A chord that has quietly stopped working is invisible everywhere
        // else: the desktop still lists the binding and nothing logs an error.
        let mechanism = brain_store::hotkey::select(&brain_store::hotkey::Host);
        let chord = brain_store::hotkey::DEFAULT_CHORD.human();
        if let Some(text) = brain_store::gesture::health(
            mechanism != brain_store::hotkey::Mechanism::None,
            now,
            ops.gesture_last_fired()?,
            ops.last_active()?,
        )
        .message(&chord)
        {
            conditions.push(Condition {
                tier: "attention",
                text,
                acknowledgeable: false,
            });
        }

        let stale = ops.stale_export_count()?;
        if stale > 0 {
            conditions.push(Condition {
                tier: "attention",
                text: format!(
                    "{stale} redaction(s) destroyed content that older export files still \
                     contain. Redaction cannot reach a backup."
                ),
                // The files are somewhere this program cannot look, so it can
                // never verify they were dealt with.
                acknowledgeable: true,
            });
        }

        // Tier two: worth knowing, no action. Described by cause — most of
        // these are a newer client writing kinds this build has no rule for,
        // which is the system working.
        let held = s.held_count()?;
        if held > 0 {
            conditions.push(Condition {
                tier: "worth-knowing",
                text: format!("{held} event(s) stored but not folded — no rule for them here"),
                acknowledgeable: false,
            });
        }
        for (_id, reason) in s.quarantined()? {
            conditions.push(Condition {
                tier: "worth-knowing",
                text: format!("an event was set aside: {reason}"),
                acknowledgeable: false,
            });
        }

        let attention = conditions.iter().filter(|c| c.tier == "attention").count();
        Ok(Health {
            notes: s.note_count()?,
            events: log.count()?,
            healthy: attention == 0,
            conditions,
            unchecked: "Signatures, body hashes and origin chains are not checked here. \
                        Run `brain verify` for that."
                .into(),
        })
    })())
}

#[tauri::command]
pub fn pending_redactions() -> std::result::Result<Vec<Pending>, String> {
    wire((|| {
        let s = read_only()?;
        let mut out = Vec::new();
        for (redact, tgt, execute_after, cancelled) in s.redact_queue()? {
            if cancelled {
                continue;
            }
            let slug = s.note(&tgt)?.map(|n| n.slug).unwrap_or_default();
            out.push(Pending {
                id: redact.0[..4].iter().map(|b| format!("{b:02x}")).collect(),
                slug,
                execute_after_ms: execute_after,
            });
        }
        Ok(out)
    })())
}

// ----------------------------------------------------------------- writes ----
//
// Each opens a session, does one thing, and drops it. Holding the writer lock
// for the lifetime of the window would stop the command line and the overlay
// from working for as long as the desktop app was open.

#[tauri::command]
pub fn edit(name: String, text: String) -> std::result::Result<(), String> {
    wire((|| brain_store::mutate::edit(&paths()?, &name, &text))())
}

#[tauri::command]
pub fn tag(name: String, tag: String) -> std::result::Result<(), String> {
    wire((|| brain_store::mutate::tag(&paths()?, &name, &tag))())
}

#[tauri::command]
pub fn untag(name: String, tag: String) -> std::result::Result<(), String> {
    wire((|| brain_store::mutate::untag(&paths()?, &name, &tag))())
}

#[tauri::command]
pub fn set_alias(name: String, alias: Option<String>) -> std::result::Result<(), String> {
    wire((|| {
        brain_store::mutate::set_alias(&paths()?, &name, alias.as_deref())
    })())
}

#[tauri::command]
pub fn pin(name: String, pinned: bool) -> std::result::Result<(), String> {
    wire((|| brain_store::mutate::pin(&paths()?, &name, pinned))())
}

#[tauri::command]
pub fn mute(name: String) -> std::result::Result<(), String> {
    wire((|| brain_store::mutate::mute_note(&paths()?, &name))())
}

#[tauri::command]
pub fn resolve(name: String) -> std::result::Result<usize, String> {
    // How many competing versions were dismissed, so the interface can say so
    // rather than implying one was chosen. Both remain in the log.
    wire((|| brain_store::mutate::resolve(&paths()?, &name))())
}

/// Queue a redaction.
///
/// The confirmation is the interface's job, not this function's — but the
/// window it opens is not: the delay, the cap and the snapshot are properties
/// of the operation and apply however it was reached.
#[tauri::command]
pub fn redact(name: String) -> std::result::Result<String, String> {
    wire((|| brain_store::mutate::redact(&paths()?, &name))())
}

#[tauri::command]
pub fn cancel_redaction(id: String) -> std::result::Result<(), String> {
    wire((|| brain_store::mutate::cancel_redaction(&paths()?, &id))())
}

#[tauri::command]
pub fn acknowledge_exports() -> std::result::Result<(), String> {
    wire((|| {
        Ops::open(&paths()?.ops)?.exports_pruned()?;
        Ok(())
    })())
}

/// Offer a session, and record that it was offered.
///
/// # Why this writes
///
/// Being offered an item is itself an outcome — `shown` — and a surface that
/// read without writing would schedule the same five notes forever. So this is
/// a write command even though it looks like a read, and that is the same
/// reason `brain review` opens the store rather than the projection.
///
/// The page holds the returned session rather than asking again, because a
/// second call really would be a second offering: `last_shown` moves, and the
/// items become ineligible for a day. In a terminal that is fine — running the
/// command twice is two deliberate acts. Clicking a nav button twice is not,
/// and a surface that quietly consumed somebody's session because they looked
/// away would be punishing them for navigating.
#[tauri::command]
pub fn review() -> std::result::Result<ReviewSession, String> {
    wire((|| {
        let mut s = Session::open(paths()?)?;
        let now = now_ms() as i64;
        let chosen = brain_store::review::session(&s, now)?;

        let items: Vec<ReviewItem> = chosen
            .iter()
            .map(|i| ReviewItem {
                slug: i.note.slug.clone(),
                // Only where nothing was promoted: once a note has a type, a
                // competing reading is noise rather than an offer.
                suggestion: match (
                    s.state.promotion_of(&i.note.tgt).ok().flatten(),
                    i.note.text.as_deref(),
                ) {
                    (None, Some(text)) => suggestion_for(text, i.note.tz.as_deref()),
                    _ => None,
                },
                text: i.note.text.clone(),
                because: match i.because {
                    brain_store::review::Because::Forward => "forward",
                    brain_store::review::Because::Never => "never-engaged",
                    brain_store::review::Because::Context => "context",
                },
            })
            .collect();

        let nagging = s
            .state
            .repeatedly_lapsed(now, 3)?
            .into_iter()
            .map(|tgt| {
                Ok(Nagging {
                    slug: s.state.slug_of(&tgt)?.unwrap_or_default(),
                })
            })
            .collect::<Result<Vec<_>>>()?;

        // Written after everything above has succeeded, so a failure to read
        // cannot spend a session nobody got to see — and in one commit, because
        // five of them is a third of a second and the pane cannot draw until
        // they are done.
        let tgts: Vec<_> = chosen.iter().map(|i| i.note.tgt).collect();
        brain_store::review::shown(&mut s, &tgts)?;

        Ok(ReviewSession { items, nagging })
    })())
}

/// What became of one offered note.
///
/// Three outcomes and no fourth. §2.7 removed the grade rather than the
/// feature: nothing here knows whether you remembered, because nothing here
/// asks.
#[tauri::command]
pub fn reviewed(name: String, outcome: String) -> std::result::Result<(), String> {
    wire((|| {
        let outcome = match outcome.as_str() {
            "engaged" => brain_core::body::Outcome::Engaged,
            "deferred" => brain_core::body::Outcome::Deferred,
            other => {
                return Err(Error::Usage(format!(
                    "'{other}' is not an outcome the page may record"
                )));
            }
        };
        brain_store::review::record(&paths()?, &name, outcome)
    })())
}

/// Say that missing an instance of a series leaves something owed.
///
/// The answer to the one question review asks. Only ever set by a person —
/// §3.6 forbids the recognizer inferring it, because a missed dose and a missed
/// walk are the same sentence to a parser.
#[tauri::command]
pub fn carries_forward(name: String) -> std::result::Result<(), String> {
    wire((|| brain_store::promote::carries_forward(&paths()?, &name))())
}

/// Drain anything that has become due.
///
/// Called on a slow tick from the page. This is the latency half of the
/// due-work rule — every process startup is the floor that makes the guarantee,
/// and a resident window is what makes a deadline land near its time.
#[tauri::command]
pub fn tick() -> std::result::Result<usize, String> {
    wire((|| {
        let mut s = Session::open(paths()?)?;
        Ok(s.drain_due()?.destroyed)
    })())
}

fn now_ms() -> u64 {
    jiff::Timestamp::now().as_millisecond().max(0) as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use brain_store::projection::NoteRow;

    fn row() -> NoteRow {
        NoteRow {
            tgt: brain_core::EventId([1; 32]),
            slug: "a-note-here".into(),
            alias: None,
            text: Some("some text".into()),
            created: (1_700_000_000_000, 0),
            updated: (1_700_000_000_000, 0),
            tz: Some("UTC".into()),
            pinned: false,
            muted_until: None,
            redacted_at: None,
            redaction_queued: false,
            parent: None,
            credential: false,
            tags: Vec::new(),
            divergences: 0,
        }
    }

    #[test]
    fn an_ordinary_note_says_so() {
        assert_eq!(Note::from(row()).state, "ordinary");
    }

    #[test]
    fn withheld_outranks_incomplete_and_is_not_destroyed() {
        // A note queued for redaction has no readable text, which is exactly
        // what "incomplete" also looks like from the row alone. Telling them
        // apart is the point: one is content that has not arrived, the other is
        // content being withheld on the way to destruction — and only the
        // second is something the reader can still cancel.
        let mut queued = row();
        queued.text = None;
        queued.redaction_queued = true;
        assert_eq!(Note::from(queued).state, "withheld");

        // Destruction still wins. Once the body is gone, "queued" is history.
        let mut both = row();
        both.text = None;
        both.redaction_queued = true;
        both.redacted_at = Some(1);
        assert_eq!(Note::from(both).state, "redacted");
    }

    #[test]
    fn destroyed_and_incomplete_are_different_states() {
        // The distinction the interface most needs to get right. A note whose
        // content has not arrived and one whose content was destroyed both have
        // no text, and telling a reader the wrong one is telling them the wrong
        // story about their own archive.
        let mut destroyed = row();
        destroyed.text = None;
        destroyed.redacted_at = Some(1_700_000_100_000);
        assert_eq!(Note::from(destroyed).state, "redacted");

        let mut withheld = row();
        withheld.text = None;
        withheld.redaction_queued = true;

        let mut incomplete = row();
        incomplete.text = None;
        assert_eq!(Note::from(incomplete).state, "incomplete");
    }

    #[test]
    fn a_diverged_note_reports_its_versions() {
        let mut n = row();
        n.divergences = 2;
        let wire = Note::from(n);
        assert_eq!(wire.state, "diverged");
        assert_eq!(wire.divergences, 2);
    }

    #[test]
    fn a_muted_note_is_still_a_note() {
        // It stays in the timeline with a marker. Muting suppresses it
        // elsewhere; a mute that removed it from the one place it can be seen
        // in context would be a deletion with a friendlier name.
        let mut n = row();
        n.muted_until = Some(0);
        let wire = Note::from(n);
        assert_eq!(wire.state, "muted");
        assert!(wire.text.is_some());
    }

    #[test]
    fn destruction_outranks_every_other_state() {
        // A destroyed note that was also muted and diverged is reported as
        // destroyed. The others describe how it is shown; this one describes
        // what happened to it.
        let mut n = row();
        n.text = None;
        n.redacted_at = Some(1_700_000_100_000);
        n.muted_until = Some(0);
        n.divergences = 3;
        assert_eq!(Note::from(n).state, "redacted");
    }

    #[test]
    fn every_state_has_a_name() {
        // A state that fell through to nothing would render as a blank marker,
        // indistinguishable from a rendering fault. Each branch is reached.
        let mut destroyed = row();
        destroyed.text = None;
        destroyed.redacted_at = Some(1);

        let mut withheld = row();
        withheld.text = None;
        withheld.redaction_queued = true;

        let mut incomplete = row();
        incomplete.text = None;

        let mut diverged = row();
        diverged.divergences = 1;

        let mut muted = row();
        muted.muted_until = Some(0);

        let named: Vec<&str> = [row(), destroyed, withheld, incomplete, diverged, muted]
            .into_iter()
            .map(|n| Note::from(n).state)
            .collect();
        assert!(named.iter().all(|s| !s.is_empty()));
        assert_eq!(
            named,
            [
                "ordinary",
                "redacted",
                "withheld",
                "incomplete",
                "diverged",
                "muted"
            ]
        );
    }

    // ------------------------------------------------------------ packing ----
    //
    // §4.4 asks for overlap "resolved by column packing" and does not say how,
    // so the how is pinned here. Every case below is one somebody's real day
    // produces, and the failure they guard against is the same one: two things
    // at the same time drawn on top of each other, so one of them is invisible.

    const H: i64 = 60 * 60 * 1000;

    /// One timed occurrence, from hour to hour.
    fn at(start: i64, end: Option<i64>) -> Occurrence {
        Occurrence {
            slug: format!("s{start}"),
            occ: start,
            start_ms: start,
            end_ms: end,
            all_day: false,
            done: false,
            shift: None,
            text: None,
            column: 0,
            columns: 1,
            due: false,
            was_due: None,
        }
    }

    fn packed(mut rows: Vec<Occurrence>) -> Vec<(i64, usize, usize)> {
        pack(&mut rows);
        rows.iter()
            .map(|o| (o.start_ms, o.column, o.columns))
            .collect()
    }

    #[test]
    fn a_day_with_no_overlap_is_one_column_wide() {
        let out = packed(vec![
            at(9 * H, Some(10 * H)),
            at(11 * H, Some(12 * H)),
            at(14 * H, Some(15 * H)),
        ]);
        for (start, col, cols) in out {
            assert_eq!(
                (col, cols),
                (0, 1),
                "the occurrence at {start} was narrowed"
            );
        }
    }

    #[test]
    fn two_things_at_once_sit_side_by_side() {
        let out = packed(vec![at(9 * H, Some(10 * H)), at(9 * H, Some(10 * H))]);
        assert_eq!(out[0].2, 2, "an overlap was not widened");
        assert_ne!(out[0].1, out[1].1, "two occurrences share a column");
    }

    #[test]
    fn touching_is_not_overlapping() {
        // A meeting ending at ten and one starting at ten do not overlap, and
        // drawing them as if they did halves the width of both for nothing.
        let out = packed(vec![at(9 * H, Some(10 * H)), at(10 * H, Some(11 * H))]);
        assert_eq!(out[0].2, 1);
        assert_eq!(out[1].2, 1);
    }

    #[test]
    fn a_column_is_reused_once_it_is_free() {
        // 9–11 beside 9–10 and 10–11: the second two follow each other, so they
        // share a column and the day is two wide rather than three.
        let out = packed(vec![
            at(9 * H, Some(11 * H)),
            at(9 * H, Some(10 * H)),
            at(10 * H, Some(11 * H)),
        ]);
        assert!(
            out.iter().all(|(_, _, cols)| *cols == 2),
            "a reusable column was not reused: {out:?}"
        );
    }

    #[test]
    fn a_cluster_does_not_narrow_the_rest_of_the_day() {
        // The reason width is per-cluster rather than per-day. A three-way
        // overlap at noon must not squeeze a lone meeting at nine into a third
        // of the page.
        let out = packed(vec![
            at(9 * H, Some(10 * H)),
            at(12 * H, Some(13 * H)),
            at(12 * H, Some(13 * H)),
            at(12 * H, Some(13 * H)),
        ]);
        assert_eq!(out[0].2, 1, "the morning was narrowed by the afternoon");
        assert!(out[1..].iter().all(|(_, _, c)| *c == 3));
    }

    #[test]
    fn a_chain_of_overlaps_is_one_cluster() {
        // A overlaps B, B overlaps C, A does not touch C. They are still one
        // cluster: B has to be drawn beside both, so all three share a width.
        let out = packed(vec![
            at(9 * H, Some(10 * H)),
            at(9 * H + H / 2, Some(11 * H)),
            at(10 * H + H / 2, Some(12 * H)),
        ]);
        assert!(
            out.iter().all(|(_, _, c)| *c == 2),
            "a transitive overlap was split: {out:?}"
        );
    }

    #[test]
    fn an_occurrence_with_no_written_end_still_collides() {
        // Most captures have no end: the promoter refuses to invent §3.3's
        // "30m default", because a wrong end time is harder to notice than a
        // missing one. A zero-length interval overlaps nothing, so without a
        // minimum these would be packed into one column and drawn on top of
        // each other — two meetings at nine, and only one of them visible.
        let out = packed(vec![at(9 * H, None), at(9 * H, None)]);
        assert_eq!(out[0].2, 2);
        assert_ne!(out[0].1, out[1].1);
    }

    #[test]
    fn a_missing_end_does_not_invent_a_collision() {
        // The other side of it. The minimum exists to make two things at the
        // same time collide, not to make an hour's worth of room.
        let out = packed(vec![at(9 * H, None), at(10 * H, None)]);
        assert_eq!(out[0].2, 1);
        assert_eq!(out[1].2, 1);
    }

    #[test]
    fn a_band_entry_is_left_where_it_is() {
        // All-day entries and due tasks have no position on an hour axis, which
        // is the whole reason §3.2 keeps them in a band above the grid. Packing
        // one would give it a column, and a column is a claim about a time.
        let mut rows = vec![at(9 * H, Some(10 * H)), at(9 * H, Some(10 * H))];
        rows[0].all_day = true;
        pack(&mut rows);
        assert_eq!((rows[0].column, rows[0].columns), (0, 1));
        assert_eq!(
            (rows[1].column, rows[1].columns),
            (0, 1),
            "the timed one was widened against a band entry it cannot overlap"
        );
    }
}
