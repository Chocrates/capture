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

use capture_store::{
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
    /// What this capture is, as the projection reads it.
    pub kind: String,
    /// The projection's rendering of the fields. Empty for an ordinary note,
    /// whose content is its text.
    ///
    /// Taken from the projection rather than formatted here. A window that
    /// composed its own description of the same fields would be a second
    /// rendering, and it would disagree with the phone's invisibly — each
    /// correct on its own screen.
    pub descriptor: String,
    /// The items this capture names, where it is a list.
    pub items: Vec<ListLine>,
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
            kind: "note".into(),
            descriptor: String::new(),
            items: Vec::new(),
            state,
        }
    }
}

/// A note, completed from the projection it came out of.
///
/// `From<NoteRow>` cannot do this — it has no store to ask — so every call
/// site that has one goes through here instead. One function rather than one
/// per site, for the reason the descriptor lives in the projection at all: two
/// readings of one note that can disagree will eventually disagree, and each
/// will look right on its own screen.
fn rendered(s: &Store, n: NoteRow) -> Result<Note> {
    let tgt = n.tgt;
    let mut out = Note::from(n);

    if let Some(p) = s.promotion_of(&tgt)? {
        out.kind = p.kind;
        out.descriptor = p.descriptor;
    }

    if s.list_tag_of(&tgt)?.is_some() {
        // This capture's own items, by one indexed query. The tag-wide view
        // answers the wider question and is a different call — asking it per
        // row costs a full tag gather per row.
        out.items = s
            .items_of(&tgt)?
            .into_iter()
            .map(|e| ListLine {
                text: e.text,
                done: e.done,
                ord: e.ord,
            })
            .collect();
    }
    Ok(out)
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
    /// Every device in the roster, and when this one last heard from it.
    pub peers: Vec<Peer>,
}

/// One device in the roster.
#[derive(Serialize)]
pub struct Peer {
    /// Enough of the key for a person to compare against another device's
    /// screen. Not the whole thing: a prefix nobody checks is worse than one
    /// short enough that they do.
    pub key: String,
    /// True for the device this surface is running on.
    pub is_self: bool,
    /// Where it can be dialled, if that is known. A peer that only ever dials
    /// *in* is demonstrably alive at no address this device can call, so the
    /// address and the last contact are separate facts.
    pub addr: Option<String>,
    /// Milliseconds since it was last reached, if ever.
    pub last_seen_ms: Option<u64>,
}

/// How long a peer may go unreached before it is worth saying so.
///
/// A week. Shorter fires every time a laptop spends a weekend shut, and a
/// report that cries wolf is one nobody believes the time it means something.
const PEER_STALE_MS: u64 = 7 * 24 * 60 * 60 * 1000;

/// A duration in the roughest terms that are still true.
fn approx(ms: u64) -> String {
    let days = ms / (24 * 60 * 60 * 1000);
    let hours = ms / (60 * 60 * 1000);
    if days > 0 {
        format!("{days} day(s)")
    } else if hours > 0 {
        format!("{hours} hour(s)")
    } else {
        format!("{} minute(s)", ms / 60_000)
    }
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
    projection::current(&p)
}

fn wire<T>(r: Result<T>) -> std::result::Result<T, String> {
    r.map_err(|e| e.to_string())
}

// ------------------------------------------------------------------ reads ----

#[tauri::command]
pub fn timeline(limit: usize) -> std::result::Result<Vec<Note>, String> {
    wire((|| {
        let s = read_only()?;
        s.timeline(limit)?
            .into_iter()
            .map(|n| rendered(&s, n))
            .collect()
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
            .map(capture_core::link::segments)
            .unwrap_or_default()
            .into_iter()
            .flat_map(|p| match p {
                // A run of plain text is split again on the mentions in it, so
                // `@sarah` is followable in the middle of a sentence. Two
                // scanners, but over different syntaxes owned by different
                // crates: links are the fold's, sigils are the recognizer's.
                capture_core::link::Piece::Text(t) => split_mentions(&t),
                capture_core::link::Piece::Link(name) => {
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
            (None, Some(text)) => {
                capture_store::review::suggestion_for(text, row.tz.as_deref(), now_ms() as i64)
            }
            _ => None,
        };

        Ok(Page {
            note: rendered(&s, row)?,
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
    let found = capture_parse::mentions(text);
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
        let kind = entity_kind(c)?;

        // Through the entity rather than the string, which is the difference
        // this change makes here. `mentioning` answers for one exact spelling;
        // after a merge a page holds what *either* spelling named, and after a
        // rename it is called something neither of them says.
        //
        // Falls back to the string where no entity has been derived for it,
        // which should not happen for `@`, `^` or `#` — but a page that came
        // back empty because an index was stale would look exactly like a page
        // with nothing on it, and those are different facts.
        match s.entity(kind, &value)? {
            Some(ent) => s
                .entity_notes(&ent.ent, now_ms())?
                .into_iter()
                .map(|n| rendered(&s, n))
                .collect(),
            None => s
                .mentioning(c, &value, now_ms())?
                .into_iter()
                .map(|n| rendered(&s, n))
                .collect(),
        }
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
        s.tagged(&tag, now_ms())?
            .into_iter()
            .map(|n| rendered(&s, n))
            .collect()
    })())
}

/// One line of a list.
#[derive(Serialize)]
pub struct ListLine {
    pub text: String,
    pub done: bool,
    /// Its place in the written order, where it has one.
    pub ord: Option<u32>,
}

/// A list: the items written under a tag, then the tasks carrying it.
///
/// The tag page and the list are the same question asked two ways — "what
/// carries this" and "what is still to do about it" — so this sits beside
/// `tagged` rather than behind a pane of its own. The proposal makes a
/// dedicated desktop list surface a non-goal: the window already has somewhere
/// to put an answer about a tag.
#[tauri::command]
pub fn list(tag: String) -> std::result::Result<Vec<ListLine>, String> {
    wire((|| {
        let s = read_only()?;
        Ok(s.list(&tag, now_ms())?
            .into_iter()
            .map(|e| ListLine {
                text: e.text,
                done: e.done,
                ord: e.ord,
            })
            .collect())
    })())
}

/// Tick an item off a list, or put it back.
///
/// `tick_item` rather than `tick`: this window already has a `tick`, and it is
/// the slow clock that drains due work. Two unrelated meanings under one name
/// in one IPC surface is the kind of collision that is obvious today and is a
/// wrong call site in six months.
#[tauri::command]
pub fn tick_item_at(name: String, ord: u32, done: bool) -> std::result::Result<String, String> {
    wire((|| {
        let paths = paths()?;
        Ok(
            match capture_store::promote::tick_at(&paths, &name, ord, done)? {
                capture_store::promote::Ticked::Done => "done".to_string(),
                capture_store::promote::Ticked::Already => "already".to_string(),
                // Unreachable: a position names exactly one item, which is the
                // whole reason this sits beside `tick_item`.
                capture_store::promote::Ticked::Ambiguous(n) => format!("ambiguous:{n}"),
                capture_store::promote::Ticked::NoSuchItem => "absent".to_string(),
            },
        )
    })())
}

/// Take the reading the archive offered but would not act on.
///
/// The other half of the confidence floor. Below it a recognition is shown and
/// applied to nothing, which is right for a machine deciding alone and wrong as
/// a final answer — the person reading it wrote the text.
///
/// Answers what it became.
#[tauri::command]
pub fn accept_suggestion(name: String) -> std::result::Result<String, String> {
    wire((|| {
        let as_ = capture_store::promote::accept(&paths()?, &name)?;
        Ok(String::from(match as_ {
            capture_core::body::Promoted::Note => "note",
            capture_core::body::Promoted::Task(_) => "task",
            capture_core::body::Promoted::Event(_) => "event",
            capture_core::body::Promoted::Series(_) => "series",
        }))
    })())
}

/// Insert the character a capture was missing to be a list.
///
/// Rewrites what somebody wrote, where accepting a type annotates and leaves it
/// alone — so the window says which character and where, rather than offering a
/// bare yes. Answers the text as it now reads.
#[tauri::command]
pub fn fix_list(name: String) -> std::result::Result<String, String> {
    wire((|| {
        capture_store::mutate::accept_near_miss(&paths()?, &name)
    })())
}

/// Tick an item by its words.
///
/// Kept for somebody typing them. A surface should use `tick_item_at`, which
/// cannot be ambiguous.
#[tauri::command]
pub fn tick_item(tag: String, item: String, done: bool) -> std::result::Result<String, String> {
    wire((|| {
        let paths = paths()?;
        Ok(
            match capture_store::promote::tick(&paths, &tag, &item, done, now_ms())? {
                capture_store::promote::Ticked::Done => "done".to_string(),
                capture_store::promote::Ticked::Already => "already".to_string(),
                capture_store::promote::Ticked::Ambiguous(n) => format!("ambiguous:{n}"),
                capture_store::promote::Ticked::NoSuchItem => "absent".to_string(),
            },
        )
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

        // Entities rather than distinct strings. The difference shows up in two
        // places somebody would otherwise report as bugs: a renamed person is
        // listed under the name they chose, and a merged-away spelling is gone
        // from the sidebar instead of sitting there with a page holding nothing.
        let listed = |sigil: char| -> Result<Vec<Entry>> {
            let kind = entity_kind(sigil)?;
            let mut rows = s.entities_of_kind(kind, now_ms())?;
            rows.sort_by(|a, b| b.count.cmp(&a.count).then(a.display.cmp(&b.display)));
            Ok(rows
                .into_iter()
                .map(|e| Entry {
                    name: e.display,
                    slug: None,
                    parent: None,
                    count: Some(e.count as u32),
                })
                .collect())
        };

        // Tags keep the older path deliberately. `distinct_mentions('#')` unions
        // the `#` written in text with the OR-Set a `tag` event maintains, and
        // an entity is derived from text alone — so listing tags as entities
        // would silently drop every tag added by hand.
        //
        // That is also the non-goal this change declared: tags already have a
        // surface and a membership set of their own, and giving them a second
        // identity with renaming and merging is a separate argument.
        let tags = s
            .distinct_mentions('#')?
            .into_iter()
            .map(|(name, count)| Entry {
                name,
                slug: None,
                parent: None,
                count: Some(count),
            })
            .collect();

        Ok(Index {
            pages,
            people: listed('@')?,
            projects: listed('^')?,
            tags,
        })
    })())
}

/// Which kind of entity a sigil names.
///
/// One mapping, shared by every command here, so the window cannot decide that
/// `^` means something different from what the store thinks it means.
fn entity_kind(sigil: char) -> Result<&'static str> {
    capture_core::id::entity_kind(sigil)
        .ok_or_else(|| Error::Usage(format!("'{sigil}' is not a mention sigil; use @, ^ or #")))
}

/// Give an entity a name of your choosing.
///
/// §8.4 makes the window the first-party surface, so identity is correctable
/// here and not only from a command line. Extraction cannot know that two
/// spellings are one person, and the person reading the page can.
#[tauri::command]
pub fn rename_entity(
    sigil: String,
    value: String,
    display: String,
) -> std::result::Result<(), String> {
    wire((|| {
        let kind = entity_kind(sigil.chars().next().unwrap_or(' '))?;
        capture_store::mutate::rename_entity(&paths()?, kind, &value, &display)
    })())
}

/// Say that two entities were one all along.
///
/// A redirection and not a rewrite: nothing moves, and either name then reaches
/// one page.
#[tauri::command]
pub fn merge_entities(
    sigil: String,
    from: String,
    into: String,
) -> std::result::Result<(), String> {
    wire((|| {
        let kind = entity_kind(sigil.chars().next().unwrap_or(' '))?;
        capture_store::mutate::merge_entities(&paths()?, kind, &from, &into)
    })())
}

/// Bind the note that *is* an entity, or unbind it.
#[tauri::command]
pub fn bind_backing(
    sigil: String,
    value: String,
    note: Option<String>,
) -> std::result::Result<(), String> {
    wire((|| {
        let kind = entity_kind(sigil.chars().next().unwrap_or(' '))?;
        capture_store::mutate::bind_backing(&paths()?, kind, &value, note.as_deref())
    })())
}

/// Put a page under another, or back at the top.
#[tauri::command]
pub fn set_parent(name: String, under: Option<String>) -> std::result::Result<(), String> {
    wire((|| {
        capture_store::mutate::set_parent(&paths()?, &name, under.as_deref())
    })())
}

/// A pairing in progress.
///
/// Held here because the exchange outlives the call that starts it: the window
/// shows a code, and the answer arrives whenever somebody walks to the other
/// device and types it. A command that blocked until then would freeze the
/// interface for up to three minutes.
#[derive(Default)]
pub struct Pairing(std::sync::Mutex<Option<Progress>>);

struct Progress {
    code: String,
    /// `None` until it finishes; then who was admitted, or why it did not.
    outcome: std::sync::Arc<std::sync::Mutex<Option<std::result::Result<String, String>>>>,
}

/// What the window shows while a pairing is open.
#[derive(Serialize)]
pub struct PairingState {
    /// The digits to read out, while one is running.
    pub code: Option<String>,
    /// Who was admitted, once it has finished.
    pub admitted: Option<String>,
    /// Why it did not, if it did not.
    pub failed: Option<String>,
}

/// Show a code, and admit whoever types it correctly.
///
/// Returns as soon as the code exists. The exchange runs on its own thread and
/// the window asks [`pairing_state`] how it went.
#[tauri::command]
pub fn start_pairing(
    pairing: tauri::State<'_, Pairing>,
) -> std::result::Result<PairingState, String> {
    wire((|| {
        let paths = paths()?;
        let offering = capture_store::pairing::Offering::open(&paths)?;
        let code = offering.shown();

        let outcome = std::sync::Arc::new(std::sync::Mutex::new(None));
        let done = std::sync::Arc::clone(&outcome);
        let started = now_ms();

        std::thread::spawn(move || {
            let result = offering
                .admit_one(&paths, started, now_ms)
                .map(|key| short_key(&key))
                .map_err(|e| e.to_string());
            if let Ok(mut slot) = done.lock() {
                *slot = Some(result);
            }
        });

        if let Ok(mut held) = pairing.0.lock() {
            *held = Some(Progress {
                code: code.clone(),
                outcome,
            });
        }

        Ok(PairingState {
            code: Some(code),
            admitted: None,
            failed: None,
        })
    })())
}

/// How the pairing is going.
#[tauri::command]
pub fn pairing_state(pairing: tauri::State<'_, Pairing>) -> PairingState {
    let Ok(held) = pairing.0.lock() else {
        return PairingState {
            code: None,
            admitted: None,
            failed: None,
        };
    };
    let Some(progress) = held.as_ref() else {
        return PairingState {
            code: None,
            admitted: None,
            failed: None,
        };
    };

    match progress.outcome.lock().ok().and_then(|o| o.clone()) {
        None => PairingState {
            code: Some(progress.code.clone()),
            admitted: None,
            failed: None,
        },
        Some(Ok(key)) => PairingState {
            code: None,
            admitted: Some(key),
            failed: None,
        },
        Some(Err(why)) => PairingState {
            code: None,
            admitted: None,
            failed: Some(why),
        },
    }
}

/// Stop accepting what a device writes, from where it is now.
///
/// The remedy for a device that was lost or stolen, and the reason it belongs
/// here rather than only in the terminal: the moment somebody needs it is not
/// a moment to be looking up a command.
///
/// It retires the **key**, not an origin — retiring an origin would let the
/// device mint a fresh incarnation on its next restart and return. Nothing is
/// deleted: a retired device's past events were legitimately authored and stay
/// valid, and discarding them would mean losing real notes.
#[tauri::command]
pub fn retire_device(key: String) -> std::result::Result<(), String> {
    wire((|| {
        let paths = paths()?;
        let identity = capture_store::Identity::load(&paths)?;

        let log = capture_store::log::Log::open_read(&paths.events)?;
        let mut roster = capture_core::roster::build(&log.all()?);
        roster.honour_self(&identity.public().to_bytes());
        drop(log);

        let wanted = key.to_ascii_lowercase();
        let matches: Vec<[u8; 32]> = roster
            .keys
            .iter()
            .copied()
            .filter(|k| short_key(k).starts_with(&wanted))
            .collect();

        let subject = match matches.as_slice() {
            [] => return Err(Error::NotFound(format!("no device matches {key}"))),
            [one] => *one,
            _ => {
                return Err(Error::Usage(
                    "more than one device starts with that — give more of the key".into(),
                ));
            }
        };

        // A roster with nobody in it can never admit anyone again, because
        // `device_add` needs a signature from a current member.
        if roster.keys.len() == 1 {
            return Err(Error::Usage(
                "this is the only device in the roster, and a roster with nobody in it can \
                 never admit anyone again"
                    .into(),
            ));
        }

        let mut s = capture_store::session::Session::open(paths.clone())?;
        s.retire(&subject, None).map(|_| ())
    })())
}

/// Change a recurring series from one occurrence onward.
///
/// The boundary is a date rather than an index, and the rule is read by the
/// same recognizer a capture uses — see the command's own documentation for
/// why a split is not an edit. This is the surface half of it: §8.4 makes the
/// window the first-party surface, so a mutation reachable only from the
/// terminal is a mutation most of its uses cannot reach.
#[tauri::command]
pub fn split_series(name: String, from: String, rule: String) -> std::result::Result<(), String> {
    wire((|| {
        let zone = jiff::tz::TimeZone::system()
            .iana_name()
            .unwrap_or("UTC")
            .to_string();
        capture_store::promote::split_at(&paths()?, &name, &from, &rule, now_ms() as i64, &zone)
            .map(|_| ())
    })())
}
/// Occurrences and one-off events on a day.
#[tauri::command]
pub fn agenda(day: String) -> std::result::Result<Vec<Occurrence>, String> {
    wire((|| {
        let s = read_only()?;
        let zone = capture_time::Zone::new("UTC").expect("UTC is always present");
        let date = capture_time::FloatingDate::parse(&day)
            .ok_or_else(|| Error::Usage(format!("'{day}' is not a date")))?;
        let from = capture_time::at_local(date, 0, 0, zone)
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
        // only the second half: a task due today was listed by `capture tasks`
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
        let today = format!("{}", capture_time::civil_date(now_ms() as i64, zone));
        if day == today {
            for tgt in s.tasks_overdue(&today)? {
                let was = s.promotion_of(&tgt)?.and_then(|p| match p.as_ {
                    capture_core::body::Promoted::Task(t) => t.due,
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

        capture_store::calendar::pack(&mut out);
        Ok(out)
    })())
}

impl capture_store::calendar::Timed for Occurrence {
    fn start_ms(&self) -> i64 {
        self.start_ms
    }

    fn end_ms(&self) -> Option<i64> {
        self.end_ms
    }

    fn on_axis(&self) -> bool {
        // All-day entries and due tasks have no position on an hour axis,
        // which is the whole reason §3.2 and §4.4 keep them in bands of their
        // own.
        !self.all_day && !self.due
    }

    fn place(&mut self, column: usize, columns: usize) {
        self.column = column;
        self.columns = columns;
    }
}

/// Mark a task done, or open again.
///
/// Not `set_occurrence`, and the difference is §3.6's: a task has state of its
/// own, where a series has state per instance and nothing accumulates between
/// them. Sending one through the other would either lose the due date or
/// invent an occurrence that no rule produced.
#[tauri::command]
pub fn set_task_done(name: String, done: bool) -> std::result::Result<(), String> {
    wire((|| {
        capture_store::promote::set_done(&paths()?, &name, done)
    })())
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
            capture_core::event::Kind::OCC_DONE
        } else {
            capture_core::event::Kind::OCC_UNDONE
        };
        s.append(kind, Some(tgt), None, |nonce| {
            let mut b = Vec::new();
            minicbor::encode(&capture_core::body::OccBody { nonce, occ }, &mut b)
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
        s.by_context(&field, &value)?
            .into_iter()
            .map(|n| rendered(&s, n))
            .collect()
    })())
}

/// What a reader has already accounted for.
///
/// Held for the lifetime of the window so that a change is reported once
/// rather than every time it is asked about.
#[derive(Default)]
pub struct Watcher(pub std::sync::Mutex<Option<capture_store::watch::Watch>>);

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
        let log = capture_store::log::Log::open_read(&p.events)?;

        let mut guard = watcher.0.lock().unwrap_or_else(|e| e.into_inner());
        match guard.as_mut() {
            Some(w) => Ok(w.poll(&log, &ops)?.any()),
            None => {
                // The first look establishes where things stand. Reporting a
                // change here would refresh the page immediately after it drew.
                *guard = Some(capture_store::watch::Watch::start(&log, &ops)?);
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
        let log = capture_store::log::Log::open_read(&p.events)?;
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
        if capture_store::spool::in_use(&p.spool)? {
            conditions.push(Condition {
                tier: "attention",
                text: "captures are being spooled — the store could not be written".into(),
                acknowledgeable: false,
            });
        }
        // A chord that has quietly stopped working is invisible everywhere
        // else: the desktop still lists the binding and nothing logs an error.
        let mechanism = capture_store::hotkey::select(&capture_store::hotkey::Host);
        let chord = capture_store::hotkey::DEFAULT_CHORD.human();
        if let Some(text) = capture_store::gesture::health(
            mechanism != capture_store::hotkey::Mechanism::None,
            now,
            ops.gesture_last_fired()?,
            // Derived from the log rather than kept beside it: every event
            // names the device that wrote it and when.
            capture_store::log::Log::open_read(&p.events)?
                .last_wall(&capture_store::identity::Identity::load(&p)?.dev())?,
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

        // Whether sync is running at all, before any peer is described.
        //
        // The window is where this matters most: somebody reading it is the
        // least likely to know whether a daemon is running, and the most likely
        // to read "never reached" as a statement about their network. It was
        // the only thing said here, and on a machine where `capture-syncd` had
        // never been started it pointed at the wrong half of the system.
        //
        // Same decision and same words as the phone and the command line, from
        // `capture-store`, because three surfaces answering this separately is
        // how two of them came to be wrong.
        // Membership from the log rather than the projection: the projection's
        // copy is only written when a gate opens, and a local capture never
        // opens one — so reading it reports an empty roster on any device that
        // has not yet ingested from a peer.
        let roster = capture_core::roster::build(&log.all()?);
        let me = capture_store::Identity::load(&p)?.public().to_bytes();
        let known = ops.peer_addresses()?;

        // Whether sync is running at all, before any peer is described.
        //
        // The window is where this matters most: somebody reading it is the
        // least likely to know whether a daemon is running, and the most likely
        // to read "never reached" as a statement about their network. That was
        // the only thing said here, and on a machine where `capture-syncd` had
        // never been started it pointed at the wrong half of the system.
        //
        // Same decision and same words as the phone and the command line, from
        // `capture-store`, because three surfaces answering this separately is
        // how two of them came to be wrong.
        let others = roster.keys.iter().filter(|k| **k != me).count();
        if let Some((tier, text)) =
            capture_store::syncstate::sync_state(&ops, others, now)?.message()
        {
            conditions.push(Condition {
                tier: if tier == "attention" {
                    "attention"
                } else {
                    "worth-knowing"
                },
                text,
                acknowledgeable: false,
            });
        }

        let mut peers = Vec::new();
        for key in &roster.keys {
            let seen = known.iter().find(|k| &k.key == key);
            peers.push(Peer {
                key: short_key(key),
                is_self: key == &me,
                addr: seen.and_then(|s| s.addr.clone()),
                last_seen_ms: seen
                    .and_then(|s| s.last_seen)
                    .map(|t| now.saturating_sub(t)),
            });
        }

        // A device gone quiet holds captures that exist on exactly one machine,
        // and the longer it goes unnoticed the more of them there are (§4.5).
        for k in &known {
            if k.key == me {
                continue;
            }
            let quiet = k
                .last_seen
                .is_none_or(|t| now.saturating_sub(t) > PEER_STALE_MS);
            if quiet {
                conditions.push(Condition {
                    tier: "attention",
                    text: match k.last_seen {
                        Some(t) => format!(
                            "device {} has not been reached in {}. Anything captured on it \
                             since then exists only there.",
                            short_key(&k.key),
                            approx(now.saturating_sub(t))
                        ),
                        None => format!("device {} has never been reached.", short_key(&k.key)),
                    },
                    acknowledgeable: false,
                });
            }
        }

        // A redaction that has not reached every device it was issued against.
        // The pending set is fixed at issue time and never read from the
        // roster, so retiring a stolen phone cannot make its redactions report
        // success.
        for (_id, outstanding) in ops.redactions_outstanding()? {
            conditions.push(Condition {
                tier: "attention",
                text: format!(
                    "a redaction has not been acknowledged by {} device(s). Until it is, \
                     the content may still exist on them.",
                    outstanding.len()
                ),
                acknowledgeable: false,
            });
        }

        let attention = conditions.iter().filter(|c| c.tier == "attention").count();
        Ok(Health {
            notes: s.note_count()?,
            events: log.count()?,
            peers,
            healthy: attention == 0,
            conditions,
            unchecked: "Signatures, body hashes and origin chains are not checked here. \
                        Run `capture verify` for that."
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
    wire((|| capture_store::mutate::edit(&paths()?, &name, &text))())
}

#[tauri::command]
pub fn tag(name: String, tag: String) -> std::result::Result<(), String> {
    wire((|| capture_store::mutate::tag(&paths()?, &name, &tag))())
}

#[tauri::command]
pub fn untag(name: String, tag: String) -> std::result::Result<(), String> {
    wire((|| capture_store::mutate::untag(&paths()?, &name, &tag))())
}

#[tauri::command]
pub fn set_alias(name: String, alias: Option<String>) -> std::result::Result<(), String> {
    wire((|| {
        capture_store::mutate::set_alias(&paths()?, &name, alias.as_deref())
    })())
}

#[tauri::command]
pub fn pin(name: String, pinned: bool) -> std::result::Result<(), String> {
    wire((|| capture_store::mutate::pin(&paths()?, &name, pinned))())
}

#[tauri::command]
pub fn mute(name: String) -> std::result::Result<(), String> {
    wire((|| capture_store::mutate::mute_note(&paths()?, &name))())
}

#[tauri::command]
pub fn resolve(name: String) -> std::result::Result<usize, String> {
    // How many competing versions were dismissed, so the interface can say so
    // rather than implying one was chosen. Both remain in the log.
    wire((|| capture_store::mutate::resolve(&paths()?, &name))())
}

/// Queue a redaction.
///
/// The confirmation is the interface's job, not this function's — but the
/// window it opens is not: the delay, the cap and the snapshot are properties
/// of the operation and apply however it was reached.
#[tauri::command]
pub fn redact(name: String) -> std::result::Result<String, String> {
    wire((|| capture_store::mutate::redact(&paths()?, &name))())
}

#[tauri::command]
pub fn cancel_redaction(id: String) -> std::result::Result<(), String> {
    wire((|| capture_store::mutate::cancel_redaction(&paths()?, &id))())
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
/// reason `capture review` opens the store rather than the projection.
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
        // Assembled in the store, because it is more than a query: it picks,
        // reads, gathers what keeps lapsing, and records that the items were
        // shown — in that order, so a failure to read cannot spend a session
        // nobody saw. The phone runs the same sequence.
        let offered = capture_store::review::open(&paths()?, now_ms() as i64)?;

        Ok(ReviewSession {
            items: offered
                .items
                .into_iter()
                .map(|i| ReviewItem {
                    slug: i.slug,
                    suggestion: i.suggestion,
                    text: i.text,
                    because: because(i.because),
                })
                .collect(),
            nagging: offered
                .nagging
                .into_iter()
                .map(|slug| Nagging { slug })
                .collect(),
        })
    })())
}

/// Why a note was offered, as the window names it.
fn because(b: capture_store::review::Because) -> &'static str {
    match b {
        capture_store::review::Because::Forward => "forward",
        capture_store::review::Because::Never => "never-engaged",
        capture_store::review::Because::Context => "context",
    }
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
            "engaged" => capture_core::body::Outcome::Engaged,
            "deferred" => capture_core::body::Outcome::Deferred,
            other => {
                return Err(Error::Usage(format!(
                    "'{other}' is not an outcome the page may record"
                )));
            }
        };
        capture_store::review::record(&paths()?, &name, outcome)
    })())
}

/// Say that missing an instance of a series leaves something owed.
///
/// The answer to the one question review asks. Only ever set by a person —
/// §3.6 forbids the recognizer inferring it, because a missed dose and a missed
/// walk are the same sentence to a parser.
#[tauri::command]
pub fn carries_forward(name: String) -> std::result::Result<(), String> {
    wire((|| {
        capture_store::promote::carries_forward(&paths()?, &name)
    })())
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

/// Enough of a key for a person to compare against another device's screen.
fn short_key(k: &[u8; 32]) -> String {
    capture_core::id::short_key(k)
}

#[cfg(test)]
mod tests {
    use super::*;
    use capture_store::projection::NoteRow;

    fn row() -> NoteRow {
        NoteRow {
            tgt: capture_core::EventId([1; 32]),
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
        capture_store::calendar::pack(&mut rows);
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
        capture_store::calendar::pack(&mut rows);
        assert_eq!((rows[0].column, rows[0].columns), (0, 1));
        assert_eq!(
            (rows[1].column, rows[1].columns),
            (0, 1),
            "the timed one was widened against a band entry it cannot overlap"
        );
    }
}
