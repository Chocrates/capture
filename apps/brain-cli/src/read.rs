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

use brain_store::{
    Error, Result, StorePaths,
    ops::Ops,
    projection::{self, Backlink, LinkRow, NoteRow, Store},
    store,
};

/// Open the current projection without needing the writer.
pub fn open(paths: &StorePaths) -> Result<Store> {
    if !store::exists(paths) {
        return Err(Error::StoreMissing(paths.root.clone()));
    }
    let generation = Ops::open_read(&paths.ops)?.generation()?;
    Store::open_read(&projection::generation_path(&paths.state, generation))
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
    let zone = brain_time::Zone::new(&local_zone()).unwrap_or_else(utc);

    let date = match day {
        Some(d) => brain_time::FloatingDate::parse(d)
            .ok_or_else(|| Error::Usage(format!("'{d}' is not a date; use YYYY-MM-DD")))?,
        None => brain_time::civil_date(now_ms(), zone),
    };

    let from = brain_time::at_local(date, 0, 0, zone)
        .ok_or_else(|| Error::Usage("that day does not exist in this zone".into()))?;
    let to = from + 24 * 60 * 60 * 1000;

    let mut rows = Vec::new();

    // All-day entries first. They are floating dates with no instant to
    // compare, so they are found by date rather than by window (§3.2).
    for tgt in store.all_day_on(&format!("{date}"))? {
        let slug = store.slug_of(&tgt)?.unwrap_or_default();
        let text = store.text_of(&tgt)?;
        rows.push((
            slug,
            projection::OccurrenceRow {
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
            text,
        ));
    }

    for (tgt, o) in store.happening_between(from, to)? {
        // Cancelled occurrences are not drawn. They are not gone either: the
        // cancellation is an ordinary event and nothing was destroyed (§4.4).
        // One that was also moved *is* drawn — a move outranks a cancel.
        if o.cancelled && o.end_ms.is_none() && o.start_ms == o.occ {
            continue;
        }
        let slug = store.slug_of(&tgt)?.unwrap_or_default();
        let text = store.text_of(&tgt)?;
        rows.push((slug, o, text));
    }

    // Tasks due on this day. §4.4's agenda is "what is due and what occurs",
    // and until this existed it was only the second half — a task due today was
    // in `brain tasks`, and invisible on the day it mattered.
    //
    // In the all-day band, because a due date has no hour and putting one on it
    // would invent a deadline nobody wrote.
    let mut due = Vec::new();
    for (tgt, done) in store.tasks_due_on(&format!("{date}"))? {
        let slug = store.slug_of(&tgt)?.unwrap_or_default();
        let text = store.text_of(&tgt)?;
        due.push((slug, text, done));
    }

    // Overdue tasks, on today only. Still open rather than lapsed: §3.6 draws
    // that line, and a task is the one thing here that genuinely carries
    // forward — it is not finished, and saying so once is not a backlog.
    let mut overdue = Vec::new();
    if date == brain_time::civil_date(now_ms(), zone) {
        for tgt in store.tasks_overdue(&format!("{date}"))? {
            let slug = store.slug_of(&tgt)?.unwrap_or_default();
            let text = store.text_of(&tgt)?;
            let d = store
                .promotion_of(&tgt)?
                .and_then(|p| match p.as_ {
                    brain_core::body::Promoted::Task(t) => t.due,
                    _ => None,
                })
                .unwrap_or_default();
            overdue.push((slug, text, d));
        }
    }

    // What is still owed, from series that carry forward. Shown on today only:
    // a debt is a thing about now, not something to redraw on every past day
    // somebody scrolls back through.
    let mut owed = Vec::new();
    if date == brain_time::civil_date(now_ms(), zone) {
        for (tgt, o) in store.outstanding(from)? {
            let slug = store.slug_of(&tgt)?.unwrap_or_default();
            let text = store.text_of(&tgt)?;
            owed.push((slug, o, text));
        }
    }

    if json {
        let mut items: Vec<String> = rows
            .iter()
            .map(|(slug, o, text)| {
                format!(
                    "{{\"slug\":{},\"start_ms\":{},\"all_day\":{},\"done\":{},\"shift\":{},\"due\":false,\"text\":{}}}",
                    quote(slug),
                    o.start_ms,
                    o.all_day,
                    o.done,
                    o.shift.as_deref().map(quote).unwrap_or_else(|| "null".into()),
                    text.as_deref().map(quote).unwrap_or_else(|| "null".into())
                )
            })
            .collect();
        // Tasks carry `due: true` rather than being a separate array, because a
        // caller asking "what is this day" wants one list — and one that
        // silently omitted what is due would be answering a narrower question
        // than it was asked.
        for (slug, text, done) in &due {
            items.push(format!(
                "{{\"slug\":{},\"start_ms\":{},\"all_day\":true,\"done\":{},\"shift\":null,\"due\":true,\"text\":{}}}",
                quote(slug),
                from,
                done,
                text.as_deref().map(quote).unwrap_or_else(|| "null".into())
            ));
        }
        say!("[{}]", items.join(","));
        return Ok(());
    }

    say!("{date}");
    if rows.is_empty() && due.is_empty() && overdue.is_empty() && owed.is_empty() {
        say!("  nothing");
        return Ok(());
    }

    for (slug, text, done) in &due {
        say!(
            "  {}  {}  {}",
            if *done { "done    " } else { "due     " },
            slug,
            text.as_deref().map(first_line).unwrap_or_default()
        );
    }

    // All-day first, in their own band. They have no hour to sort by, and
    // mixing them into the timed list would imply one.
    for (slug, o, text) in rows.iter().filter(|(_, o, _)| o.all_day) {
        say!("  all day   {}  {}", slug, line(text.as_deref(), o));
    }
    for (slug, o, text) in rows.iter().filter(|(_, o, _)| !o.all_day) {
        let (h, m) = brain_time::civil_clock(o.start_ms, zone);
        say!("  {h:02}:{m:02}     {}  {}", slug, line(text.as_deref(), o));
    }

    if !overdue.is_empty() {
        say!();
        say!("Still open");
        for (slug, text, d) in &overdue {
            say!(
                "  was due {d}  {}  {}",
                slug,
                text.as_deref().map(first_line).unwrap_or_default()
            );
        }
    }

    if !owed.is_empty() {
        say!();
        say!("Still owed");
        for (slug, o, text) in &owed {
            let d = brain_time::civil_date(o.start_ms, zone);
            say!(
                "  from {d}  {}  {}",
                slug,
                text.as_deref().map(first_line).unwrap_or_default()
            );
        }
    }
    Ok(())
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
    let rows = store.promoted("task", 500)?;

    if json {
        let items: Vec<String> = rows
            .iter()
            .filter_map(|(tgt, descriptor)| {
                let slug = store.slug_of(tgt).ok().flatten()?;
                Some(format!(
                    "{{\"slug\":{},\"descriptor\":{}}}",
                    quote(&slug),
                    quote(descriptor)
                ))
            })
            .collect();
        say!("[{}]", items.join(","));
        return Ok(());
    }

    if rows.is_empty() {
        say!("No tasks.");
        say!();
        say!("A capture becomes one on its own where a verb and a day agree —");
        say!("\"call the dentist tomorrow\". A verb with no day is a suggestion");
        say!("and nothing more, so say it yourself with: brain task <name> [due]");
        return Ok(());
    }
    for (tgt, descriptor) in rows {
        let slug = store.slug_of(&tgt)?.unwrap_or_default();
        let text = store.text_of(&tgt)?;
        say!(
            "  {:<28}  {:<22}  {}",
            slug,
            descriptor,
            text.as_deref().map(first_line).unwrap_or_default()
        );
    }
    Ok(())
}

/// Every note naming a person or a project, or carrying a tag.
///
/// §4.3's entity page, in a terminal. One verb for the three sigils: they come
/// from different tables and mean different things underneath — a tag is an
/// OR-Set with its own events, a mention is part of the text — and none of that
/// is the reader's to keep track of while following one.
///
/// The sigil is required rather than guessed. `brain mentions sarah` could mean
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

    if json {
        let items: Vec<String> = rows
            .iter()
            .map(|n| {
                format!(
                    "{{\"slug\":{},\"text\":{}}}",
                    quote(&n.slug),
                    n.text
                        .as_deref()
                        .map(quote)
                        .unwrap_or_else(|| "null".into())
                )
            })
            .collect();
        say!("[{}]", items.join(","));
        return Ok(());
    }

    if rows.is_empty() {
        say!("Nothing names {name}.");
        say!();
        say!("Pages are not created — they exist for whatever your notes say.");
        say!("Write {name} in a capture and this one fills itself in.");
        return Ok(());
    }

    say!("{name}");
    say!();
    for n in &rows {
        say!(
            "  {:<40}  {}",
            n.slug,
            n.text.as_deref().map(first_line).unwrap_or_default()
        );
    }
    Ok(())
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
    let mut s = brain_store::session::Session::open(paths.clone())?;
    let now = now_ms();
    let items = brain_store::review::session(&s, now)?;

    // One prompt, not an inference. §3.6: a series that has lapsed repeatedly
    // might be one that genuinely accumulates — a dose rather than a walk — and
    // the recognizer must never decide that from phrasing. Asking once is the
    // difference between noticing and guessing.
    let nagging = if json {
        Vec::new()
    } else {
        s.state.repeatedly_lapsed(now, 3)?
    };

    if json {
        let out: Vec<String> = items
            .iter()
            .map(|i| {
                format!(
                    "{{\"slug\":{},\"because\":{},\"text\":{}}}",
                    quote(&i.note.slug),
                    quote(match i.because {
                        brain_store::review::Because::Forward => "forward",
                        brain_store::review::Because::Never => "never-engaged",
                        brain_store::review::Because::Context => "context",
                    }),
                    i.note
                        .text
                        .as_deref()
                        .map(quote)
                        .unwrap_or_else(|| "null".into())
                )
            })
            .collect();
        say!("[{}]", out.join(","));
    } else if items.is_empty() {
        say!("Nothing to review.");
        say!();
        say!("That is the ordinary state. Review offers what is due, and when");
        say!("nothing is, the session is over before it started.");
    } else {
        say!("{} note(s), and then this is over.", items.len());
        say!();
        for i in &items {
            let why = match i.because {
                brain_store::review::Because::Forward => "you said to come back to this",
                brain_store::review::Because::Never => "never looked at since capture",
                brain_store::review::Because::Context => "captured somewhere like here",
            };
            say!("  {}", i.note.slug);
            say!(
                "    {}",
                i.note.text.as_deref().map(first_line).unwrap_or_default()
            );
            say!("    — {why}");
            say!();
        }
        say!("brain engaged <name>   did something with it");
        say!("brain deferred <name>  push it away (twice and it stops being offered)");
    }

    for tgt in &nagging {
        let slug = s.state.slug_of(tgt)?.unwrap_or_default();
        say!();
        say!("  {slug} has lapsed several times.");
        say!("  If missing it leaves something owed — a dose, an invoice — say so:");
        say!("    brain carries-forward {slug}");
        say!("  Otherwise there is nothing to do. A missed day costs almost nothing.");
    }

    // Being offered is itself the outcome `shown`. Recorded after the items are
    // printed, so a failure to write cannot lose the session somebody is
    // already reading.
    for i in &items {
        brain_store::review::write(&mut s, i.note.tgt, brain_core::body::Outcome::Shown)?;
    }
    Ok(())
}

/// Show what the recognizer makes of some text.
///
/// Stores nothing and decides nothing. It exists because the confidence bands
/// were reasoned rather than measured (§3.1.5), and the only honest way to find
/// out whether they are right is to point them at real captures and look — a
/// band that is wrong is invisible from the inside.
pub fn explain(text: &str) -> Result<()> {
    let zone = brain_time::Zone::new(&local_zone()).unwrap_or_else(utc);
    let r = brain_parse::recognize(text, now_ms(), zone);

    say!("{text}");
    say!();

    let shape = match &r.shape {
        brain_parse::Shape::Nothing => "nothing temporal".to_string(),
        brain_parse::Shape::Task { due: None } => "a task, by its verb".to_string(),
        brain_parse::Shape::Task { due: Some(d) } => format!("a task, due {d}"),
        brain_parse::Shape::Point(brain_parse::When::Date(d)) => format!("a day: {d}"),
        brain_parse::Shape::Point(brain_parse::When::Moment(ms)) => {
            let (h, m) = brain_time::civil_clock(*ms, zone);
            format!(
                "a moment: {} at {h:02}:{m:02}",
                brain_time::civil_date(*ms, zone)
            )
        }
        brain_parse::Shape::Range { from, to } => format!("a span: {from} to {to}"),
        brain_parse::Shape::Recurring { rule, at_minute } => match at_minute {
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
        if r.confidence >= brain_store::promote::FLOOR {
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

    if json {
        say!(
            "[{}]",
            notes.iter().map(note_json).collect::<Vec<_>>().join(",")
        );
        return Ok(());
    }

    if notes.is_empty() {
        say!("Nothing was captured while that was on.");
        say!();
        say!("Either it has not happened yet, or nothing was written down during it.");
        return Ok(());
    }

    say!("Captured while this was happening:");
    say!();
    for n in &notes {
        say!(
            "  {:<30}  {}",
            n.slug,
            n.text.as_deref().map(first_line).unwrap_or_default()
        );
    }
    Ok(())
}

/// Every if-then plan still standing.
pub fn intents(paths: &StorePaths, json: bool) -> Result<()> {
    let store = open(paths)?;
    let rows = store.intents()?;

    if json {
        let items: Vec<String> = rows
            .iter()
            .map(|i| {
                format!(
                    "{{\"cue\":{},\"match\":{},\"at\":{},\"then\":{}}}",
                    quote(&i.cue_type),
                    i.cue_match
                        .as_deref()
                        .map(quote)
                        .unwrap_or_else(|| "null".into()),
                    i.cue_at
                        .map(|a| a.to_string())
                        .unwrap_or_else(|| "null".into()),
                    quote(&i.then_do)
                )
            })
            .collect();
        say!("[{}]", items.join(","));
        return Ok(());
    }

    if rows.is_empty() {
        say!("No plans.");
        say!();
        say!("  brain intent at 2026-09-01T09:00  ring the dentist");
        say!("  brain intent branch main          run the migration check");
        return Ok(());
    }

    for i in &rows {
        let when = match (i.cue_type.as_str(), &i.cue_match, i.cue_at) {
            ("time", _, Some(at)) => format!("at {}", stamp(at as u64, None)),
            ("git_branch", Some(b), _) => format!("on branch {b}"),
            (t, Some(m), _) => format!("{t} {m}"),
            (t, None, _) => t.to_string(),
        };
        say!("  {:<34}  {}", when, i.then_do);
    }
    Ok(())
}

fn utc() -> brain_time::Zone {
    brain_time::Zone::new("UTC").expect("UTC is always present")
}

fn local_zone() -> String {
    std::env::var("TZ")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| {
            std::fs::read_link("/etc/localtime").ok().and_then(|p| {
                p.to_str()
                    .and_then(|s| s.split("zoneinfo/").nth(1))
                    .map(String::from)
            })
        })
        .unwrap_or_else(|| "UTC".into())
}

fn now_ms() -> i64 {
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
    let notes = store.timeline(limit)?;

    if json {
        say!(
            "[{}]",
            notes.iter().map(note_json).collect::<Vec<_>>().join(",")
        );
        return Ok(());
    }

    if notes.is_empty() {
        empty_state();
        return Ok(());
    }

    // One column width for the whole listing, from the longest name actually
    // present. A fixed width either wraps on long names or wastes half the
    // terminal on short ones, and slugs vary by more than either allows for.
    let width = name_width(&notes);

    let mut current_day = String::new();
    for n in &notes {
        let day = day_of(n);
        if day != current_day {
            if !current_day.is_empty() {
                say!();
            }
            say!("{day}");
            current_day = day;
        }
        say!("  {}", row(n, width));
    }
    Ok(())
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

    let mechanism = brain_store::hotkey::select(&brain_store::hotkey::Host);
    if mechanism == brain_store::hotkey::Mechanism::None {
        say!("From a terminal — type the text, then press Ctrl-B:");
        say!("  eval \"$(brain shell-init zsh)\"     (or bash, in ~/.bashrc)");
        say!();
        say!("Or directly:");
        say!("  brain + your first note");
        return;
    }

    say!(
        "Press {} anywhere, type, and press Enter:",
        brain_store::hotkey::DEFAULT_CHORD.human()
    );
    say!("  brain-capture --register     sets the chord up");
    say!("  brain-capture --run          keeps the window ready");
    say!();
    say!("Or from a terminal:");
    say!("  brain + your first note");
}

/// One listing line: the name, then the text, then the state.
fn row(n: &NoteRow, width: usize) -> String {
    let mut marks = Vec::new();
    if n.pinned {
        marks.push("pinned".to_string());
    }
    if let Some(at) = n.redacted_at {
        marks.push(format!("redacted {}", date_of(at, n.tz.as_deref())));
    }
    if n.muted_until.is_some() {
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

    if json {
        say!("{}", note_json(&n));
        return Ok(());
    }

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
            say!("It is still recoverable: brain redact --cancel <id>");
            say!("Listed by: brain status");
        }
        None => say!("(no readable text)"),
    }

    say!();
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
        // Reported, never prompted. A divergence is information about the note,
        // not a task the reader has to clear before continuing.
        say!(
            "versions  {} independent, none superseded",
            n.divergences + 1
        );
    }

    let out = store.links_from(&n.tgt)?;
    let back = store.links_to(&n.tgt)?;
    if !out.is_empty() || !back.is_empty() {
        say!();
        print_links(&out, &back);
    }
    Ok(())
}

// ---------------------------------------------------------------- links ----

/// What a note points at, and what points at it.
pub fn links(paths: &StorePaths, name: &str, json: bool) -> Result<()> {
    let store = open(paths)?;
    let n = find(&store, name)?;
    let out = store.links_from(&n.tgt)?;
    let back = store.links_to(&n.tgt)?;

    if json {
        say!(
            "{{\"note\":{},\"links\":[{}],\"backlinks\":[{}]}}",
            quote(&n.slug),
            out.iter().map(link_json).collect::<Vec<_>>().join(","),
            back.iter().map(backlink_json).collect::<Vec<_>>().join(",")
        );
        return Ok(());
    }

    if out.is_empty() && back.is_empty() {
        say!("{} has no links in either direction.", n.slug);
        return Ok(());
    }
    print_links(&out, &back);
    Ok(())
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
    let now = jiff::Timestamp::now().as_millisecond().max(0) as u64;
    let hits = store.search(query, now)?;

    let rows: Vec<NoteRow> = hits
        .iter()
        .filter_map(|t| store.note(t).ok().flatten())
        .collect();

    if json {
        say!(
            "[{}]",
            rows.iter().map(note_json).collect::<Vec<_>>().join(",")
        );
        return Ok(());
    }

    if rows.is_empty() {
        say!("Nothing matches {query}.");
        return Ok(());
    }
    let width = name_width(&rows);
    for n in &rows {
        say!("{}", row(n, width));
    }
    Ok(())
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

fn stamp(ms: u64, tz: Option<&str>) -> String {
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
//
// Written by hand rather than derived. These structures cross into scripts, and
// a serialiser attached to the storage types would make every future field
// rename a silent breaking change to someone's pipeline.

fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn opt(s: Option<&str>) -> String {
    match s {
        Some(v) => quote(v),
        None => "null".to_string(),
    }
}

fn note_json(n: &NoteRow) -> String {
    format!(
        "{{\"slug\":{},\"alias\":{},\"text\":{},\"captured_ms\":{},\"updated_ms\":{},\"tz\":{},\
          \"tags\":[{}],\"pinned\":{},\"muted\":{},\"redacted\":{},\"divergences\":{}}}",
        quote(&n.slug),
        opt(n.alias.as_deref()),
        opt(n.text.as_deref()),
        n.created.0,
        n.updated.0,
        opt(n.tz.as_deref()),
        n.tags
            .iter()
            .map(|t| quote(t))
            .collect::<Vec<_>>()
            .join(","),
        n.pinned,
        n.muted_until.is_some(),
        n.is_redacted(),
        n.divergences
    )
}

fn link_json(l: &LinkRow) -> String {
    format!(
        "{{\"name\":{},\"resolved\":{},\"broken\":{}}}",
        quote(&l.name),
        opt(l.target_slug.as_deref()),
        l.is_broken()
    )
}

fn backlink_json(b: &Backlink) -> String {
    format!("{{\"slug\":{},\"as\":{}}}", quote(&b.slug), quote(&b.name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_strings_are_escaped() {
        assert_eq!(quote("plain"), "\"plain\"");
        assert_eq!(quote("say \"hi\""), "\"say \\\"hi\\\"\"");
        assert_eq!(quote("a\nb"), "\"a\\nb\"");
        assert_eq!(quote("back\\slash"), "\"back\\\\slash\"");
    }

    #[test]
    fn control_characters_do_not_escape_the_string() {
        // Capture is verbatim, so a note may contain anything at all. A raw
        // control byte in the output would produce JSON a consumer rejects.
        let out = quote("bell\u{7}here");
        assert!(out.contains("\\u0007"), "got {out}");
    }

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
