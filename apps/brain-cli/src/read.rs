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

/// Find a note by name, or say so.
fn find(store: &Store, name: &str) -> Result<NoteRow> {
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
