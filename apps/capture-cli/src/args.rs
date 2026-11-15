//! Turning a command line into an intention.
//!
//! # Why this is not a derive macro
//!
//! Everything after `+` is capture text, including things that look exactly
//! like flags. `capture + -n 5 items left` is a note about five items, and any
//! parser that gets a look at `-n` first will either consume it or refuse the
//! whole line. The capture path therefore takes the rest of the arguments
//! verbatim and never inspects them.
//!
//! That is a small enough requirement that hand-written parsing is clearer than
//! configuring a parser to make an exception for it, and the exception is the
//! part worth being able to read.
//!
//! # An unrecognised word is never a capture
//!
//! `capture buy milk` is refused, and it is refused *loudly*, with the capture
//! form spelled out. Treating unknown input as text would be friendlier for
//! exactly one keystroke and wrong forever after: `capture stauts` becomes a note
//! reading "stauts", `capture serach cats` becomes a note reading "serach cats",
//! and the archive slowly fills with typo-shaped debris that nothing marks as
//! accidental.
//!
//! The sigil is what makes the boundary legible. `+` means "this is content";
//! everything else means "this is an instruction", and an instruction that is
//! not understood is an error rather than a guess.

/// What the user asked for.
/// One occurrence of a series, as somebody named it.
///
/// By its date, which is how people name one — "this Monday's" — or by the
/// instant the rule produced, which is what the log calls it (§3.5) and what
/// scripts already pass. Turned into the instant when the command runs,
/// against the store, because only the store knows which days the series has.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OccRef {
    Ms(i64),
    Date(String),
}

/// Where a moved occurrence goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MoveTo {
    /// Exact instants, in milliseconds.
    Ms { start_ms: i64, end_ms: Option<i64> },
    /// Text such as "thursday 10am", read the way a capture is.
    Text(String),
}

/// Read an occurrence's name: milliseconds, or a `YYYY-MM-DD` date.
fn occ_ref(command: &str, v: &str) -> Result<OccRef, Bad> {
    if let Ok(ms) = v.parse::<i64>() {
        return Ok(OccRef::Ms(ms));
    }
    if capture_time::FloatingDate::parse(v).is_some() {
        return Ok(OccRef::Date(v.to_string()));
    }
    Err(Bad::Value {
        command: command.into(),
        problem: format!("{v:?} names no occurrence; give its date, like 2026-10-12"),
    })
}

/// Which stretch of days `capture cal` shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CalSpan {
    /// A month, as `YYYY-MM`; this one when absent.
    Month(Option<String>),
    /// The Monday-to-Sunday week containing a day; this one when absent.
    Week(Option<String>),
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Command {
    /// Capture. Carries the text source rather than the text, because reading
    /// from a terminal is a side effect and this module performs none.
    Capture(Source),
    /// Create a store.
    Init,
    /// Report on the store.
    Status,
    /// Every check, as codes that can be read aloud.
    Doctor {
        ask: bool,
    },
    /// Print the shell snippet that installs the capture chord.
    ShellInit(String),
    /// Notes, newest first, grouped by capture day.
    Timeline {
        limit: Option<usize>,
        json: bool,
    },
    /// One note in full.
    Show {
        name: String,
        json: bool,
    },
    /// What occurs and what is due, for a day.
    Agenda {
        day: Option<String>,
        json: bool,
    },
    /// A month or a week, and what happens on each day of it.
    Cal {
        span: CalSpan,
        json: bool,
    },
    /// Notes promoted to tasks.
    Tasks {
        json: bool,
    },
    /// Read whatever has not been read yet.
    Settle {
        json: bool,
    },
    /// Every note naming a person, a project, or carrying a tag.
    Mentions {
        name: String,
        json: bool,
    },
    /// Everyone the archive knows, by how much is said about them.
    People {
        json: bool,
    },
    /// Every project the archive knows.
    Projects {
        json: bool,
    },
    /// Events stored but not folded, and why.
    Held {
        json: bool,
    },
    /// Let a redaction the ceiling held go ahead.
    ReleaseRedaction {
        id: String,
    },
    /// Copies kept before a redaction, and bringing one back.
    Recover {
        /// Which redaction, by the prefix it printed. None lists them.
        id: Option<String>,
        json: bool,
    },
    /// An entity's page: what it is called, what backs it, what names it.
    Entity {
        name: String,
        json: bool,
    },
    /// Give an entity a name of your choosing.
    EntityRename {
        name: String,
        display: String,
    },
    /// Say that two entities were one all along.
    EntityMerge {
        from: String,
        into: String,
    },
    /// Bind the note that *is* this entity, or unbind it.
    EntityBind {
        name: String,
        note: Option<String>,
    },
    List {
        tag: String,
        json: bool,
    },
    Due {
        json: bool,
    },
    Tick {
        tag: String,
        item: String,
        done: bool,
    },
    /// Insert the character a capture was missing to be a list.
    FixList {
        name: String,
    },
    /// Say whether a bare hour meant am or pm.
    Hour {
        name: String,
        pm: bool,
    },
    /// Say which of two days a capture meant: 0 the earlier, 1 the later.
    Day {
        name: String,
        which: usize,
    },
    /// Undo a correction: read the text again.
    Reread {
        name: String,
    },
    /// Start a page with a title, optionally under another.
    NewPage {
        title: String,
        under: Option<String>,
    },
    /// Start the page that is a person, project or tag.
    NewPageFor {
        title: String,
        of: String,
    },
    /// Make a capture a page.
    MakePage {
        name: String,
    },
    /// Tick one item of a capture, by its number.
    TickAt {
        name: String,
        ord: u32,
        done: bool,
    },
    /// Put a page under another in the wiki, or back at the top.
    Parent {
        name: String,
        under: Option<String>,
    },
    /// Abandon a plan.
    Forget {
        id: String,
    },
    /// Assign the task type by hand, with an optional due date.
    Task {
        name: String,
        due: Option<String>,
    },
    /// Mark a task done, or open again.
    TaskDone {
        name: String,
        done: bool,
    },
    /// A bounded review session.
    Review {
        json: bool,
    },
    /// Make an if-then plan.
    Intent {
        cue: String,
        arg: String,
        then: String,
    },
    /// Every plan still standing.
    Intents {
        json: bool,
    },
    /// Notes captured while a recurring thing was happening.
    During {
        name: String,
        json: bool,
    },
    /// Show what the recognizer makes of some text, without storing anything.
    Read {
        text: String,
    },
    /// Say that missing an instance of a series leaves something owed.
    CarriesForward {
        name: String,
    },
    /// Record what became of a resurfaced note.
    Reviewed {
        name: String,
        outcome: String,
    },
    /// Mark one occurrence of a recurring series complete, or not.
    Occurrence {
        /// The series.
        name: String,
        /// Which occurrence.
        occ: OccRef,
        /// Whether it is complete.
        done: bool,
    },
    /// Move one occurrence of a series to a different time.
    Move {
        /// The series.
        name: String,
        /// Which occurrence.
        occ: OccRef,
        /// Where it goes.
        to: MoveTo,
    },
    /// Give one occurrence of a series its own details.
    Amend {
        /// The series.
        name: String,
        /// Which occurrence.
        occ: OccRef,
        /// A title for this occurrence only.
        title: Option<String>,
        /// A location for this occurrence only.
        location: Option<String>,
        /// Notes on this occurrence only.
        notes: Option<String>,
    },
    /// Skip one occurrence of a series, or put it back.
    Skip {
        /// The series.
        name: String,
        /// Which occurrence.
        occ: OccRef,
        /// Whether it is skipped.
        skipped: bool,
    },
    /// Notes matching a query.
    Search {
        query: String,
        json: bool,
    },
    /// What a note points at, and what points at it.
    Links {
        name: String,
        json: bool,
    },
    /// Replace a note's text with a new version.
    Edit {
        name: String,
        /// The new text: the words given, or standard input with `--stdin`.
        text: Source,
        /// The version it replaces, as `show --json` gives it. Refused, with
        /// nothing written, when the note has moved on.
        expect: Option<String>,
    },
    /// Add lines to the end of a note, losing nothing written before.
    Append {
        name: String,
        text: Source,
    },
    /// Enrol an agent on this device.
    AgentAdd {
        name: String,
    },
    /// Every agent the roster knows.
    AgentList {
        json: bool,
    },
    /// Print a skill and a permission allowlist for a harness.
    AgentInstructions {
        name: String,
    },
    /// Add a tag.
    Tag {
        name: String,
        tag: String,
    },
    /// Retract a tag.
    Untag {
        name: String,
        tag: String,
    },
    /// Assign or clear a chosen name.
    Alias {
        name: String,
        alias: Option<String>,
    },
    /// Pin or unpin.
    Pin {
        name: String,
        pinned: bool,
    },
    /// Suppress something from surfacing.
    Mute {
        what: MuteTarget,
        /// The day the mute ends, as written. Resolved to an instant where
        /// there is a clock and a zone to resolve it against — this layer
        /// checks the shape and not the meaning, so it stays testable without
        /// either.
        until: Option<String>,
    },
    /// Lift a mute.
    Unmute {
        what: MuteTarget,
    },
    /// Stop every redaction issued since a moment.
    RedactCancelSince {
        since: u64,
    },
    /// Dismiss a divergence.
    Resolve {
        name: String,
    },
    /// Every event that touched a note.
    Log {
        name: String,
        json: bool,
    },
    /// Events set aside as unusable.
    Quarantine {
        json: bool,
    },
    /// Events that failed authentication.
    Rejected {
        json: bool,
    },
    /// Recompute the projection from the log.
    Rebuild,
    /// Record a preference, or restore its default.
    Set {
        key: String,
        value: Option<String>,
    },
    /// Check every id, signature, body hash and origin chain.
    Verify {
        json: bool,
    },
    /// Acknowledge that stale export files were dealt with by hand.
    ExportsPruned,
    /// Acknowledge a device admitted by some other device.
    AdmissionSeen {
        key: String,
    },
    /// Every device in the roster, and when this one last reached it.
    Peers {
        json: bool,
    },
    /// Remember an address to sync with, for a network without discovery.
    PeersAdd {
        addr: String,
    },
    /// Admit another device, or join the roster of one.
    Pair {
        code: Option<String>,
        at: Option<String>,
    },
    /// Change a recurring series from one occurrence onward.
    Split {
        name: String,
        from: String,
        rule: String,
    },
    /// Stop accepting anything a device authors from now on.
    Retire {
        key: String,
        force: bool,
    },
    /// Vouch for another device's re-admission.
    Cosign {
        blob: String,
    },
    /// Re-admit this device, with another's signature.
    Readmit {
        cosign: Option<String>,
        by: Option<String>,
    },
    /// Write everything appended since the last export.
    Export {
        json: bool,
    },
    /// Rebuild a store from exported events.
    Restore {
        from: String,
        ignore_redactions_after: Option<u64>,
    },
    /// Clear a note's credential flag so it is exported again.
    Unflag {
        name: String,
    },
    /// Destroy a note's content, after a cancellable window.
    Redact {
        name: String,
        force: bool,
        /// Take the notes linking to it as well (§1.1 limit 5).
        with_links: bool,
    },
    /// Stop a queued redaction.
    RedactCancel {
        id: String,
    },
    /// Print usage.
    Help,
    /// Print the version.
    Version,
}

/// What a mute applies to, as the command line expresses it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum MuteTarget {
    Note(String),
    Tag(String),
    Days(String, String),
    /// Everything naming a person or a project.
    Entity(String, String),
}

/// Where a capture's text is to come from.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Source {
    /// Supplied on the command line, already joined.
    Words(String),
    /// Read from standard input, because `--stdin` was given.
    Stdin,
    /// Nothing was supplied. Whether this becomes a read from a pipe or a
    /// prompt depends on whether there is a terminal attached, which is a
    /// question about the process rather than about the arguments.
    Unspecified,
}

/// Why a command line could not be understood.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Bad {
    /// A word that is not a command. Carries it, so the message can quote it.
    UnknownCommand(String),
    /// A flag this command does not take.
    UnknownFlag { command: String, flag: String },
    /// The command is right but something it was given is not.
    ///
    /// Separate from an unknown flag because the advice differs: an unknown
    /// flag is often text someone meant to capture, and a bad value is not.
    /// Offering the capture form for a malformed timestamp would be noise.
    Value { command: String, problem: String },
    /// Nothing at all.
    Empty,
}

impl Bad {
    /// The message a person sees.
    ///
    /// Every variant names the capture form. Someone who typed the wrong thing
    /// is usually trying to write something down, and the reply that helps is
    /// the one that shows how — not a list of subcommands they must read to
    /// find it.
    pub fn message(&self) -> String {
        match self {
            Bad::UnknownCommand(w) => format!(
                "'{w}' is not a command.\n\
                 \n\
                 To capture it as a note, put a + first:\n    \
                 capture + {w} ...\n\
                 \n\
                 For the list of commands: capture help"
            ),
            Bad::UnknownFlag { command, flag } => format!(
                "'{command}' does not take {flag}.\n\
                 \n\
                 If you meant to capture that text, put a + first — everything \
                 after + is taken literally:\n    \
                 capture + {flag}\n\
                 \n\
                 For the list of commands: capture help"
            ),
            Bad::Value { command, problem } => format!(
                "capture {command}: {problem}.\n\n\
                 For how to use it: capture help"
            ),
            Bad::Empty => "Nothing to do.\n\
                 \n\
                 To capture a note:\n    \
                 capture + your note here\n\
                 \n\
                 For the list of commands: capture help"
                .to_string(),
        }
    }
}

/// Parse arguments, excluding the program name.
pub fn parse<I, S>(args: I) -> Result<Command, Bad>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let args: Vec<String> = args.into_iter().map(Into::into).collect();
    let Some(first) = args.first() else {
        return Err(Bad::Empty);
    };

    // Asking for help is never asking to do the thing. `capture pair --help`
    // opened a pairing window on a real archive, because `pair` treated every
    // word that was not its code as nothing. Checked here, ahead of every
    // command, so a command that forgets to look cannot be the one that acts.
    //
    // Not after `--`, and never for `+`: there the words are the note's.
    if first != "+"
        && args[1..]
            .iter()
            .take_while(|a| *a != "--")
            .any(|a| a == "--help" || a == "-h")
    {
        return Ok(Command::Help);
    }

    match first.as_str() {
        "+" => Ok(Command::Capture(source(&args[1..]))),
        "init" => flagless("init", &args[1..], Command::Init),
        "status" => flagless("status", &args[1..], Command::Status),
        "doctor" => match &args[1..] {
            [] => Ok(Command::Doctor { ask: false }),
            [a] if a == "--ask" => Ok(Command::Doctor { ask: true }),
            rest => flagless("doctor", rest, Command::Doctor { ask: false }),
        },
        "timeline" => {
            let rest = &args[1..];
            let json = has_json(rest);
            let mut limit = None;
            for a in rest.iter().filter(|a| *a != "--json") {
                match a.parse::<usize>() {
                    Ok(n) => limit = Some(n),
                    Err(_) => {
                        return Err(Bad::UnknownFlag {
                            command: "timeline".into(),
                            flag: a.clone(),
                        });
                    }
                }
            }
            Ok(Command::Timeline { limit, json })
        }
        // `capture cal [YYYY-MM]`, `capture cal --week [YYYY-MM-DD]`.
        "cal" => {
            let rest: Vec<&String> = args[1..].iter().filter(|a| *a != "--json").collect();
            let json = has_json(&args[1..]);
            let span = match rest.as_slice() {
                [] => CalSpan::Month(None),
                [w] if *w == "--week" => CalSpan::Week(None),
                [w, d] if *w == "--week" => CalSpan::Week(Some((*d).clone())),
                [m] if !m.starts_with('-') => CalSpan::Month(Some((*m).clone())),
                [.., extra] => {
                    return Err(Bad::UnknownFlag {
                        command: "cal".into(),
                        flag: (*extra).clone(),
                    });
                }
            };
            Ok(Command::Cal { span, json })
        }
        "agenda" => {
            let rest = &args[1..];
            let json = has_json(rest);
            let days: Vec<&String> = rest.iter().filter(|a| *a != "--json").collect();
            match days.as_slice() {
                [] => Ok(Command::Agenda { day: None, json }),
                [d] => Ok(Command::Agenda {
                    day: Some((*d).clone()),
                    json,
                }),
                [_, extra, ..] => Err(Bad::UnknownFlag {
                    command: "agenda".into(),
                    flag: (*extra).clone(),
                }),
            }
        }
        "intent" => {
            let rest = &args[1..];
            match rest {
                [cue, arg, then @ ..] if !then.is_empty() => Ok(Command::Intent {
                    cue: cue.clone(),
                    arg: arg.clone(),
                    then: then.join(" "),
                }),
                _ => Err(Bad::Value {
                    command: "intent".into(),
                    problem: "expected: capture intent <at|branch> <value> <what to do>".into(),
                }),
            }
        }
        "carries-forward" => match &args[1..] {
            [name] => Ok(Command::CarriesForward { name: name.clone() }),
            _ => Err(Bad::Value {
                command: "carries-forward".into(),
                problem: "expected one series".into(),
            }),
        },
        "read" => {
            let text = args[1..].join(" ");
            if text.is_empty() {
                return Err(Bad::UnknownFlag {
                    command: "read".into(),
                    flag: "nothing to read".into(),
                });
            }
            Ok(Command::Read { text })
        }
        "during" => one_name(&args[1..], "during", |name, json| Command::During {
            name,
            json,
        }),
        "intents" => {
            let rest = &args[1..];
            let json = has_json(rest);
            match rest.iter().find(|a| *a != "--json") {
                Some(extra) => Err(Bad::UnknownFlag {
                    command: "intents".into(),
                    flag: extra.clone(),
                }),
                None => Ok(Command::Intents { json }),
            }
        }
        "review" => {
            let rest = &args[1..];
            let json = has_json(rest);
            match rest.iter().find(|a| *a != "--json") {
                Some(extra) => Err(Bad::UnknownFlag {
                    command: "review".into(),
                    flag: extra.clone(),
                }),
                None => Ok(Command::Review { json }),
            }
        }
        "engaged" | "deferred" | "shown" => Ok(Command::Reviewed {
            name: args.get(1).cloned().ok_or_else(|| Bad::Value {
                command: args[0].clone(),
                problem: "no note name".into(),
            })?,
            outcome: args[0].clone(),
        }),
        "tasks" => {
            let rest = &args[1..];
            let json = has_json(rest);
            match rest.iter().find(|a| *a != "--json") {
                Some(extra) => Err(Bad::UnknownFlag {
                    command: "tasks".into(),
                    flag: extra.clone(),
                }),
                None => Ok(Command::Tasks { json }),
            }
        }
        "settle" => {
            let rest = &args[1..];
            let json = has_json(rest);
            match rest.iter().find(|a| *a != "--json") {
                Some(extra) => Err(Bad::UnknownFlag {
                    command: "settle".into(),
                    flag: extra.clone(),
                }),
                None => Ok(Command::Settle { json }),
            }
        }
        "done" | "undone" => {
            let done = args[0] == "done";
            let rest = &args[1..];
            match rest {
                // One name is a task; a name and an occurrence is one instance
                // of a series. Both are "this is finished", and §3.6 keeps them
                // distinct underneath — a task has state, a series has state
                // per instance and nothing accumulates between them.
                [name] => Ok(Command::TaskDone {
                    name: name.clone(),
                    done,
                }),
                [name, occ] => Ok(Command::Occurrence {
                    name: name.clone(),
                    occ: occ_ref(&args[0], occ)?,
                    done,
                }),
                _ => Err(Bad::Value {
                    command: args[0].clone(),
                    problem: "expected a task, or a series and an occurrence".into(),
                }),
            }
        }
        // `capture mentions @sarah`, `^acme`, `#reading`. One verb for three
        // sigils, because a reader following one is doing the same thing in
        // each case — the difference between an OR-Set tag and a mention in the
        // text is real and is not theirs to keep track of.
        "mentions" => one_name(&args[1..], "mentions", |name, json| Command::Mentions {
            name,
            json,
        }),
        // `capture people`, `capture projects`. The index the desktop sidebar
        // has always drawn and no command could answer — `distinct_mentions`
        // existed with no caller at all.
        "people" => json_only(&args[1..], "people", |json| Command::People { json }),
        "projects" => json_only(&args[1..], "projects", |json| Command::Projects { json }),
        // `capture entity @sarah`. The page, rather than only the notes on it:
        // what it is called, every spelling that reaches it, and the note that
        // *is* it where one is bound.
        "entity" => one_name(&args[1..], "entity", |name, json| Command::Entity {
            name,
            json,
        }),
        // Correcting what extraction could not know. §8.4's gradient is the
        // reason these are here rather than only in the window: a misparse seen
        // on one surface must be fixable from it.
        "entity-rename" => match &args[1..] {
            [name, display] => Ok(Command::EntityRename {
                name: name.clone(),
                display: display.clone(),
            }),
            _ => Err(Bad::Value {
                command: "entity-rename".into(),
                problem: "expected entity-rename <@name> <display>".into(),
            }),
        },
        "entity-merge" => match &args[1..] {
            [from, into] => Ok(Command::EntityMerge {
                from: from.clone(),
                into: into.clone(),
            }),
            _ => Err(Bad::Value {
                command: "entity-merge".into(),
                problem: "expected entity-merge <@from> <@into>".into(),
            }),
        },
        "entity-bind" => match &args[1..] {
            [name] => Ok(Command::EntityBind {
                name: name.clone(),
                note: None,
            }),
            [name, note] => Ok(Command::EntityBind {
                name: name.clone(),
                note: Some(note.clone()),
            }),
            _ => Err(Bad::Value {
                command: "entity-bind".into(),
                problem: "expected entity-bind <@name> [note]   (no note unbinds)".into(),
            }),
        },
        // `capture tick-at <capture> 2`. By number, which is what a surface
        // knows and what words cannot answer when two items read the same.
        //
        // One-based on the way in, because the list prints itself that way and
        // somebody counting rows on a screen is counting from one.
        // `capture fix-list <note>`. Named for what it does to the text, not
        // "accept": this rewrites what somebody wrote, where accepting a type
        // leaves it alone, and the two should not sound the same.
        "fix-list" => one_name(&args[1..], "fix-list", |name, _| Command::FixList { name }),
        // `capture hour <note> am|pm`. Answers the question a bare hour
        // raised, by writing the answer into the text.
        "hour" => match &args[1..] {
            [name, half] if half == "am" || half == "pm" => Ok(Command::Hour {
                name: name.clone(),
                pm: half == "pm",
            }),
            _ => Err(Bad::Value {
                command: "hour".into(),
                problem: "give the note and am or pm: capture hour <note> pm".into(),
            }),
        },
        // `capture day <note> 1|2`. Answers which of two days was meant, as
        // `capture show` numbers them, by writing the day into the text.
        "day" => match &args[1..] {
            [name, n] if n == "1" || n == "2" => Ok(Command::Day {
                name: name.clone(),
                which: if n == "1" { 0 } else { 1 },
            }),
            _ => Err(Bad::Value {
                command: "day".into(),
                problem: "give the note and 1 or 2, as capture show numbers the days: capture day <note> 2".into(),
            }),
        },
        // `capture reread <note>`. Takes back a correction — an accepted
        // suggestion, a kind set by hand — so the text decides again.
        "reread" => one_name(&args[1..], "reread", |name, _| Command::Reread { name }),
        // `capture page new <title> [--under <page> | --of @x]` starts a page;
        // `capture page <note>` makes a capture one. A page is never read for
        // dates or types; `capture unpage` hands it back to the recognizer.
        "page" => match &args[1..] {
            [new, rest @ ..] if new == "new" => {
                // Refused rather than half-applied (pages-that-hold-up G15):
                // the page of a person sits on their page, not in the tree, so
                // "under" has nowhere to put it.
                if rest.iter().any(|a| a == "--under") && rest.iter().any(|a| a == "--of") {
                    return Err(Bad::Value {
                        command: "page".into(),
                        problem: "a page is either under another page (--under) or someone's own \
                                  page (--of), not both"
                            .into(),
                    });
                }
                let flag = rest.iter().position(|a| a == "--under" || a == "--of");
                let (title, flagged) = match flag {
                    Some(i) => match rest.get(i + 1) {
                        Some(u) if i + 2 == rest.len() => {
                            (&rest[..i], Some((rest[i].as_str(), u.clone())))
                        }
                        _ => {
                            return Err(Bad::Value {
                                command: "page".into(),
                                problem: "--under takes one page and --of one @person, ^project \
                                          or #tag, after the title"
                                    .into(),
                            });
                        }
                    },
                    None => (rest, None),
                };
                if title.is_empty() {
                    return Err(Bad::Value {
                        command: "page".into(),
                        problem: "give the page a title: capture page new Car maintenance".into(),
                    });
                }
                let title = title.join(" ");
                Ok(match flagged {
                    Some(("--of", of)) => Command::NewPageFor { title, of },
                    under => Command::NewPage {
                        title,
                        under: under.map(|(_, u)| u),
                    },
                })
            }
            [name] => Ok(Command::MakePage { name: name.clone() }),
            _ => Err(Bad::Value {
                command: "page".into(),
                problem: "capture page new <title>, or capture page <note>".into(),
            }),
        },
        "unpage" => one_name(&args[1..], "unpage", |name, _| Command::Reread { name }),
        "tick-at" | "untick-at" => {
            let done = args[0] == "tick-at";
            match &args[1..] {
                [name, n] => match n.parse::<u32>() {
                    Ok(n) if n >= 1 => Ok(Command::TickAt {
                        name: name.clone(),
                        ord: n - 1,
                        done,
                    }),
                    _ => Err(Bad::Value {
                        command: args[0].clone(),
                        problem: format!("'{n}' is not an item number; they start at 1"),
                    }),
                },
                _ => Err(Bad::Value {
                    command: args[0].clone(),
                    problem: "expected a capture and an item number".into(),
                }),
            }
        }
        // `capture list groceries`. The tag without its sigil, because a
        // leading `#` is a comment to most shells and quoting it is a thing to
        // remember at exactly the wrong moment.
        // `capture due`. What would fire, and when — without firing it.
        //
        // The answer to "why did nothing happen", which is the question this
        // feature generates more than any other. A surface that could only be
        // checked by waiting for it is one nobody can debug.
        "due" => match &args[1..] {
            [] => Ok(Command::Due { json: false }),
            [f] if f == "--json" => Ok(Command::Due { json: true }),
            [extra, ..] => Err(Bad::UnknownFlag {
                command: "due".into(),
                flag: extra.clone(),
            }),
        },
        "list" => one_name(&args[1..], "list", |tag, json| Command::List {
            tag: tag.trim_start_matches('#').to_string(),
            json,
        }),
        // `capture tick groceries milk`, and its undo.
        //
        // A verb of its own rather than `capture done groceries milk`: `done`
        // already takes `<name>` and `<name> <occ>`, and telling those apart
        // from a list and an item by whether the second argument parses as a
        // date is the fragile disambiguation §3.1.5's guards exist to avoid.
        verb @ ("tick" | "untick") => match &args[1..] {
            [tag, item] => Ok(Command::Tick {
                tag: tag.trim_start_matches('#').to_string(),
                item: item.clone(),
                done: verb == "tick",
            }),
            [] | [_] => Err(Bad::Value {
                command: verb.into(),
                problem: "say which list and which item:  capture tick groceries milk".into(),
            }),
            [_, _, extra, ..] => Err(Bad::UnknownFlag {
                command: verb.into(),
                flag: extra.clone(),
            }),
        },
        // `capture parent <page> <under>` files it; `capture parent <page>` puts it
        // back at the top. The second form is deliberately the short one: it is
        // the undo, and an undo that is harder to type than the thing it undoes
        // is one people do not reach for.
        // §2.4 rates being easy to walk away from as part of the case for
        // having intents at all — and until now there was no way to walk away
        // from one from any surface, though the fold has always known how.
        "forget" => match &args[1..] {
            [id] => Ok(Command::Forget { id: id.clone() }),
            _ => Err(Bad::Value {
                command: "forget".into(),
                problem: "expected the id of a plan, from `capture intents`".into(),
            }),
        },
        "parent" => {
            let rest = &args[1..];
            match rest {
                [name] => Ok(Command::Parent {
                    name: name.clone(),
                    under: None,
                }),
                [name, under] => Ok(Command::Parent {
                    name: name.clone(),
                    under: Some(under.clone()),
                }),
                _ => Err(Bad::Value {
                    command: "parent".into(),
                    problem: "expected a page, and optionally what to put it under".into(),
                }),
            }
        }
        "task" => {
            let rest = &args[1..];
            match rest {
                [name] => Ok(Command::Task {
                    name: name.clone(),
                    due: None,
                }),
                [name, due] => Ok(Command::Task {
                    name: name.clone(),
                    due: Some(due.clone()),
                }),
                _ => Err(Bad::Value {
                    command: "task".into(),
                    problem: "expected a note, and optionally a due date".into(),
                }),
            }
        }
        "show" => one_name(&args[1..], "show", |name, json| Command::Show {
            name,
            json,
        }),
        "links" => one_name(&args[1..], "links", |name, json| Command::Links {
            name,
            json,
        }),
        "search" => {
            let rest = &args[1..];
            let json = has_json(rest);
            let terms: Vec<String> = rest.iter().filter(|a| *a != "--json").cloned().collect();
            if terms.is_empty() {
                return Err(Bad::Value {
                    command: "search".into(),
                    problem: "no search terms".into(),
                });
            }
            Ok(Command::Search {
                query: terms.join(" "),
                json,
            })
        }
        "edit" => {
            let (name, text, expect) = text_command("edit", &args[1..], true)?;
            Ok(Command::Edit { name, text, expect })
        }
        "append" => {
            let (name, text, _) = text_command("append", &args[1..], false)?;
            Ok(Command::Append { name, text })
        }
        "agent" => match &args[1..] {
            [add, name] if add == "add" => Ok(Command::AgentAdd { name: name.clone() }),
            [list, rest @ ..] if list == "list" => match rest {
                [] => Ok(Command::AgentList { json: false }),
                [j] if j == "--json" => Ok(Command::AgentList { json: true }),
                [other, ..] => Err(Bad::UnknownFlag {
                    command: "agent list".into(),
                    flag: other.clone(),
                }),
            },
            [ins, rest @ ..] if ins == "instructions" => match rest {
                [] => Ok(Command::AgentInstructions {
                    name: "claude".into(),
                }),
                [flag, name] if flag == "--name" => Ok(Command::AgentInstructions {
                    name: name.clone(),
                }),
                [other, ..] => Err(Bad::UnknownFlag {
                    command: "agent instructions".into(),
                    flag: other.clone(),
                }),
            },
            _ => Err(Bad::Value {
                command: "agent".into(),
                problem: "agent add <name> | agent list | agent instructions [--name <name>]"
                    .into(),
            }),
        },
        "tag" => name_and_rest(&args[1..], "tag", |name, rest| Command::Tag {
            name,
            tag: rest.join(" "),
        }),
        "untag" => name_and_rest(&args[1..], "untag", |name, rest| Command::Untag {
            name,
            tag: rest.join(" "),
        }),
        "alias" => {
            let rest = &args[1..];
            match rest {
                [name] => Ok(Command::Alias {
                    name: name.clone(),
                    alias: None,
                }),
                [name, alias] => Ok(Command::Alias {
                    name: name.clone(),
                    alias: Some(alias.clone()),
                }),
                _ => Err(Bad::Value {
                    command: "alias".into(),
                    problem: "expected a note and an optional new name".into(),
                }),
            }
        }
        "pin" => one_name(&args[1..], "pin", |name, _| Command::Pin {
            name,
            pinned: true,
        }),
        "unpin" => one_name(&args[1..], "unpin", |name, _| Command::Pin {
            name,
            pinned: false,
        }),
        "mute" | "unmute" => {
            let lifting = args[0] == "unmute";
            let rest = &args[1..];

            // A trailing `until <day>` is taken off first, so every target
            // form below keeps working unchanged and none of them has to know
            // about it. Parsing it into the arms instead would mean four more
            // patterns and four chances for them to disagree.
            let (rest, until) = match rest {
                [front @ .., kw, day] if kw == "until" => {
                    if lifting {
                        return Err(Bad::Value {
                            command: "unmute".into(),
                            problem: "lifting a mute takes no 'until'; \
                                      to change when one ends, mute it again"
                                .into(),
                        });
                    }
                    if !looks_like_a_day(day) {
                        return Err(Bad::Value {
                            command: "mute".into(),
                            problem: format!("'{day}' is not a day; use YYYY-MM-DD"),
                        });
                    }
                    (front, Some(day.clone()))
                }
                // `until` with nothing after it, which is a half-written
                // command rather than a target called "until".
                [.., kw] if kw == "until" => {
                    return Err(Bad::Value {
                        command: "mute".into(),
                        problem: "expected a day after 'until', as YYYY-MM-DD".into(),
                    });
                }
                _ => (rest, None),
            };

            let built = |what: MuteTarget| {
                if lifting {
                    Command::Unmute { what }
                } else {
                    Command::Mute {
                        what,
                        until: until.clone(),
                    }
                }
            };

            match rest {
                // A tag, recognised by the sigil it is written with everywhere
                // else. Anything else is a note's name.
                //
                // `#` stays a *tag* mute rather than an entity one. Both exist
                // and they are not the same scope: a tag mute covers anything
                // carrying the tag now or later, which is what somebody writing
                // `#` means, and the entity scope is what the entity page acts
                // on. Changing `#` here would silently move an existing
                // command onto a different scope.
                [one] if one.starts_with('#') => Ok(built(MuteTarget::Tag(
                    one.trim_start_matches('#').to_string(),
                ))),
                // A person or a project. Everything naming them, through the
                // entity — so a rename or a merge carries the mute with it,
                // which matching on the spelling could not.
                [one] if one.starts_with('@') || one.starts_with('^') => {
                    let mut c = one.chars();
                    let sigil = c.next().unwrap_or(' ');
                    let name: String = c.collect();
                    match capture_core::id::entity_kind(sigil) {
                        Some(kind) if !name.is_empty() => {
                            Ok(built(MuteTarget::Entity(kind.to_string(), name)))
                        }
                        _ => Err(Bad::Value {
                            command: if lifting { "unmute" } else { "mute" }.into(),
                            problem: format!("'{one}' names nothing after the sigil"),
                        }),
                    }
                }
                [one] => Ok(built(MuteTarget::Note(one.clone()))),
                [from, to] if looks_like_a_day(from) && looks_like_a_day(to) => {
                    Ok(built(MuteTarget::Days(from.clone(), to.clone())))
                }
                _ => Err(Bad::Value {
                    command: if lifting { "unmute" } else { "mute" }.into(),
                    problem: "expected a note, a #tag, an @person or ^project, \
                              or two dates as YYYY-MM-DD"
                        .into(),
                }),
            }
        }
        "resolve" => one_name(&args[1..], "resolve", |name, _| Command::Resolve { name }),
        "log" => one_name(&args[1..], "log", |name, json| Command::Log { name, json }),
        "move" => match &args[1..] {
            // `capture move gym 2026-10-12 --to "thursday 10am"`: how a person
            // says it. First, because the four-argument millisecond form below
            // would otherwise take `--to` for an end time.
            [name, occ, to, text] if to == "--to" => Ok(Command::Move {
                name: name.clone(),
                occ: occ_ref(&args[0], occ)?,
                to: MoveTo::Text(text.clone()),
            }),
            // Named exactly as `capture done` and `capture skip` name one.
            [name, occ, start] | [name, occ, start, _] => {
                let bad = |v: &String| Bad::UnknownFlag {
                    command: args[0].clone(),
                    flag: v.clone(),
                };
                let start_ms = start.parse::<i64>().map_err(|_| bad(start))?;
                let end_ms = match &args[1..] {
                    [_, _, _, end] => Some(end.parse::<i64>().map_err(|_| bad(end))?),
                    _ => None,
                };
                Ok(Command::Move {
                    name: name.clone(),
                    occ: occ_ref(&args[0], occ)?,
                    to: MoveTo::Ms { start_ms, end_ms },
                })
            }
            _ => Err(Bad::Value {
                command: args[0].clone(),
                problem: "expected a series, the date of the occurrence and where it goes, \
                          like: capture move gym 2026-10-12 --to \"thursday 10am\""
                    .into(),
            }),
        },
        "amend" => {
            let rest = &args[1..];
            let flag = |name: &str| {
                rest.iter()
                    .position(|a| a == name)
                    .and_then(|i| rest.get(i + 1))
                    .cloned()
            };
            let positional: Vec<&String> = {
                let mut out = Vec::new();
                let mut skip_next = false;
                for a in rest {
                    if skip_next {
                        skip_next = false;
                        continue;
                    }
                    if a.starts_with("--") {
                        skip_next = true;
                        continue;
                    }
                    out.push(a);
                }
                out
            };
            match positional.as_slice() {
                [name, occ] => {
                    let occ_ms = occ_ref(&args[0], occ)?;
                    let (title, location, notes) =
                        (flag("--title"), flag("--location"), flag("--notes"));
                    // An amendment that overrides nothing is a request that
                    // meant something and said nothing. Refused rather than
                    // written: an empty exception folds to no change at all.
                    if title.is_none() && location.is_none() && notes.is_none() {
                        return Err(Bad::Value {
                            command: args[0].clone(),
                            problem: "expected at least one of --title, --location or --notes"
                                .into(),
                        });
                    }
                    Ok(Command::Amend {
                        name: (*name).clone(),
                        occ: occ_ms,
                        title,
                        location,
                        notes,
                    })
                }
                _ => Err(Bad::Value {
                    command: args[0].clone(),
                    problem: "expected a series and an occurrence, as its original start \
                              in milliseconds — capture show <series> lists them"
                        .into(),
                }),
            }
        }
        "skip" | "unskip" => {
            let skipped = args[0] == "skip";
            match &args[1..] {
                // Named exactly as `capture done` names one, rather than a
                // second convention for two of the five exceptions.
                [name, occ] => Ok(Command::Skip {
                    name: name.clone(),
                    occ: occ_ref(&args[0], occ)?,
                    skipped,
                }),
                _ => Err(Bad::Value {
                    command: args[0].clone(),
                    problem: "expected a series and the date of the occurrence, \
                              like: capture skip gym 2026-10-12"
                        .into(),
                }),
            }
        }
        "quarantine" => Ok(Command::Quarantine {
            json: has_json(&args[1..]),
        }),
        "held" => Ok(Command::Held {
            json: has_json(&args[1..]),
        }),
        "recover" => {
            let rest = &args[1..];
            let json = has_json(rest);
            // Named by the same prefix `redact --cancel` takes, so the two
            // recovery routes — inside the window and after it — are reached
            // the same way.
            let id = rest.iter().find(|a| *a != "--json").cloned();
            Ok(Command::Recover { id, json })
        }
        "rejected" => Ok(Command::Rejected {
            json: has_json(&args[1..]),
        }),
        "rebuild" => flagless("rebuild", &args[1..], Command::Rebuild),
        "export" => Ok(Command::Export {
            json: has_json(&args[1..]),
        }),
        "unflag" => one_name(&args[1..], "unflag", |name, _| Command::Unflag { name }),
        "redact" => {
            let rest = &args[1..];
            // Beside `--cancel` rather than a command of its own: both are
            // things you do to a redaction by the id it printed, and putting
            // them together is what makes the pair findable.
            if let Some(i) = rest.iter().position(|a| a == "--release") {
                let Some(id) = rest.get(i + 1) else {
                    return Err(Bad::Value {
                        command: "redact".into(),
                        problem: "--release needs the id the redaction printed, \
                                  as capture status lists it"
                            .into(),
                    });
                };
                return Ok(Command::ReleaseRedaction { id: id.clone() });
            }
            if let Some(i) = rest.iter().position(|a| a == "--cancel") {
                if rest.get(i + 1).map(String::as_str) == Some("--all-since") {
                    let Some(v) = rest.get(i + 2) else {
                        return Err(Bad::Value {
                            command: "redact".into(),
                            problem: "--all-since needs a time".into(),
                        });
                    };
                    let Some(ms) = parse_time(v) else {
                        return Err(Bad::Value {
                            command: "redact".into(),
                            problem: format!("cannot read '{v}' as a time"),
                        });
                    };
                    return Ok(Command::RedactCancelSince { since: ms });
                }
                let Some(id) = rest.get(i + 1) else {
                    return Err(Bad::Value {
                        command: "redact".into(),
                        problem: "--cancel needs the id of a queued redaction".into(),
                    });
                };
                return Ok(Command::RedactCancel { id: id.clone() });
            }
            let force = rest.iter().any(|a| a == "--force");
            let with_links = rest.iter().any(|a| a == "--with-links");
            let names: Vec<&String> = rest.iter().filter(|a| !a.starts_with("--")).collect();
            match names.as_slice() {
                [name] => Ok(Command::Redact {
                    name: (*name).clone(),
                    force,
                    with_links,
                }),
                _ => Err(Bad::Value {
                    command: "redact".into(),
                    problem: "expected one note to destroy".into(),
                }),
            }
        }
        "restore" => {
            let rest = &args[1..];
            let mut from = None;
            let mut cutoff = None;
            let mut i = 0;
            while i < rest.len() {
                match rest[i].as_str() {
                    "--ignore-redactions-after" => {
                        let Some(v) = rest.get(i + 1) else {
                            return Err(Bad::Value {
                                command: "restore".into(),
                                problem: "--ignore-redactions-after needs a time".into(),
                            });
                        };
                        let Some(ms) = parse_time(v) else {
                            return Err(Bad::Value {
                                command: "restore".into(),
                                problem: format!(
                                    "cannot read '{v}' as a time — give milliseconds since the \
                                     epoch, or an RFC 3339 stamp like 2026-08-21T14:30:00Z"
                                ),
                            });
                        };
                        cutoff = Some(ms);
                        i += 2;
                    }
                    other => {
                        from = Some(other.to_string());
                        i += 1;
                    }
                }
            }
            match from {
                Some(from) => Ok(Command::Restore {
                    from,
                    ignore_redactions_after: cutoff,
                }),
                None => Err(Bad::Value {
                    command: "restore".into(),
                    problem: "no export directory given".into(),
                }),
            }
        }
        "pair" => {
            let rest = &args[1..];
            let at_index = rest.iter().position(|a| a == "--at");
            // Opening a window another device can join is not something to do
            // on a mistyped flag.
            let is_at_value = |i: usize| at_index.is_some_and(|at| i == at + 1);
            if let Some((_, flag)) = rest
                .iter()
                .enumerate()
                .find(|(i, a)| a.starts_with('-') && *a != "--at" && !is_at_value(*i))
            {
                return Err(Bad::UnknownFlag {
                    command: "pair".into(),
                    flag: flag.clone(),
                });
            }
            let at = at_index.and_then(|i| rest.get(i + 1).cloned());
            let code = rest.iter().find(|a| !a.starts_with('-')).cloned();
            let code = match (&code, &at) {
                // `--at`'s value is not the code.
                (Some(c), Some(a)) if c == a => None,
                _ => code,
            };
            Ok(Command::Pair { code, at })
        }
        "retire" => match args[1..].iter().find(|a| !a.starts_with('-')) {
            Some(key) => Ok(Command::Retire {
                key: key.clone(),
                force: args[1..].iter().any(|a| a == "--yes"),
            }),
            None => Err(Bad::Value {
                command: "retire".into(),
                problem: "give the key shown in capture peers".into(),
            }),
        },
        "split" => {
            let rest = &args[1..];
            let after = |flag: &str| {
                rest.iter()
                    .position(|a| a == flag)
                    .and_then(|i| rest.get(i + 1).cloned())
            };
            let name = rest.iter().find(|a| !a.starts_with('-')).cloned();
            match (name, after("--from"), after("--to")) {
                (Some(name), Some(from), Some(rule)) if name != from && name != rule => {
                    Ok(Command::Split { name, from, rule })
                }
                _ => Err(Bad::Value {
                    command: "split".into(),
                    problem: "capture split NAME --from 2026-09-15 --to \"every tuesday at 9\""
                        .into(),
                }),
            }
        }
        "cosign" => match args[1..].iter().find(|a| !a.starts_with('-')) {
            Some(blob) => Ok(Command::Cosign { blob: blob.clone() }),
            None => Err(Bad::Value {
                command: "cosign".into(),
                problem: "give what the other device printed".into(),
            }),
        },
        "readmit" => {
            let rest = &args[1..];
            let after = |flag: &str| {
                rest.iter()
                    .position(|a| a == flag)
                    .and_then(|i| rest.get(i + 1).cloned())
            };
            Ok(Command::Readmit {
                cosign: after("--cosign"),
                by: after("--by"),
            })
        }
        "peers" if args.get(1).is_some_and(|a| a == "add") => match &args[2..] {
            [addr] if is_host_port(addr) => Ok(Command::PeersAdd { addr: addr.clone() }),
            _ => Err(Bad::Value {
                command: "peers add".into(),
                problem: "give an address as HOST:PORT, e.g. 192.168.1.20:7878".into(),
            }),
        },
        "peers" => Ok(Command::Peers {
            json: has_json(&args[1..]),
        }),
        "exports-pruned" => flagless("exports-pruned", &args[1..], Command::ExportsPruned),
        "admission-seen" => match &args[1..] {
            [key] => Ok(Command::AdmissionSeen { key: key.clone() }),
            _ => Err(Bad::Value {
                command: "admission-seen".into(),
                // `capture status` is the one that shows it. There is no
                // `capture health` — being told to run something that answers
                // "is not a command" ends the search in the wrong place.
                problem: "give the key shown in capture status".into(),
            }),
        },
        "verify" => Ok(Command::Verify {
            json: has_json(&args[1..]),
        }),
        "set" => match &args[1..] {
            [key] => Ok(Command::Set {
                key: key.clone(),
                value: None,
            }),
            [key, rest @ ..] if !rest.is_empty() => Ok(Command::Set {
                key: key.clone(),
                value: Some(rest.join(" ")),
            }),
            _ => Err(Bad::Value {
                command: "set".into(),
                problem: "expected a setting and an optional value".into(),
            }),
        },
        "shell-init" => match args.get(1) {
            Some(shell) => Ok(Command::ShellInit(shell.clone())),
            None => Err(Bad::Value {
                command: "shell-init".into(),
                problem: "no shell (try: bash, zsh)".into(),
            }),
        },
        "help" | "--help" | "-h" => Ok(Command::Help),
        "version" | "--version" | "-V" => Ok(Command::Version),
        other => Err(Bad::UnknownCommand(other.to_string())),
    }
}

/// Work out where a capture's text comes from.
///
/// Deliberately total: there is no invalid capture invocation. `+` followed by
/// anything at all is a request to store that anything, and the only way to
/// fail is for the resulting text to be empty — which is decided after reading,
/// not here.
fn source(rest: &[String]) -> Source {
    match rest.first().map(String::as_str) {
        None => Source::Unspecified,

        // The one flag the capture path recognises, and it is recognised only
        // in first position. Later on it is text: `capture + note the --stdin
        // flag` is a note about a flag.
        Some("--stdin") if rest.len() == 1 => Source::Stdin,

        // Everything after `--` is literal, including a leading `--stdin` or
        // anything else that would otherwise be read as a flag. This is the
        // escape hatch that makes the rule above safe: the flag can always be
        // captured as text by someone who means it.
        Some("--") => Source::Words(join(&rest[1..])),

        _ => Source::Words(join(rest)),
    }
}

/// Rejoin arguments the shell already split.
///
/// This is lossy and unavoidably so. The shell has consumed quoting, collapsed
/// runs of whitespace, expanded variables and globs, and stripped comments
/// before this process started; nothing here can put any of it back. A capture
/// typed as `capture + costs   $40 #urgent` arrives having already lost the
/// spacing, the `$40`, and everything from the `#`.
///
/// That is the reason the keybinding exists rather than a shell alias: it sends
/// the raw line before the shell touches it. This path stays for scripts and
/// for text with nothing surprising in it.
fn join(words: &[String]) -> String {
    words.join(" ")
}

/// Read a cutoff as milliseconds since the epoch.
///
/// Accepts a plain millisecond count or an RFC 3339 timestamp. A bare date is
/// deliberately not accepted: "before 2026-08-21" is ambiguous about which
/// midnight in which zone, and this flag decides whether destroyed content
/// comes back.
fn parse_time(v: &str) -> Option<u64> {
    if let Ok(ms) = v.parse::<u64>() {
        return Some(ms);
    }
    v.parse::<jiff::Timestamp>()
        .ok()
        .map(|t| t.as_millisecond().max(0) as u64)
}

/// Whether a word is shaped like `YYYY-MM-DD`.
fn looks_like_a_day(v: &str) -> bool {
    v.len() == 10
        && v.as_bytes()[4] == b'-'
        && v.as_bytes()[7] == b'-'
        && v.chars().filter(char::is_ascii_digit).count() == 8
}

/// `HOST:PORT` with a port that is a port. The host is not looked up here:
/// a name that does not resolve today may on the network it is meant for.
fn is_host_port(s: &str) -> bool {
    s.rsplit_once(':').is_some_and(|(host, port)| {
        !host.is_empty()
            && !host.contains(char::is_whitespace)
            && port.parse::<u16>().is_ok_and(|p| p > 0)
    })
}

fn has_json(rest: &[String]) -> bool {
    rest.iter().any(|a| a == "--json")
}

/// Commands shaped `capture <verb> [--json]` and nothing else.
fn json_only(rest: &[String], verb: &str, build: impl Fn(bool) -> Command) -> Result<Command, Bad> {
    let json = has_json(rest);
    match rest.iter().find(|a| *a != "--json") {
        Some(extra) => Err(Bad::UnknownFlag {
            command: verb.into(),
            flag: extra.clone(),
        }),
        None => Ok(build(json)),
    }
}

/// Commands shaped `capture <verb> <name> [--json]`.
fn one_name(
    rest: &[String],
    verb: &str,
    build: impl Fn(String, bool) -> Command,
) -> Result<Command, Bad> {
    let json = has_json(rest);
    let names: Vec<&String> = rest.iter().filter(|a| *a != "--json").collect();
    match names.as_slice() {
        [name] => Ok(build((*name).clone(), json)),
        [] => Err(Bad::Value {
            command: verb.into(),
            problem: "no note name".into(),
        }),
        [_, extra, ..] => Err(Bad::UnknownFlag {
            command: verb.into(),
            flag: (*extra).clone(),
        }),
    }
}

/// Commands shaped `brain <verb> <name> <the rest, verbatim>`.
///
/// The remainder is joined rather than parsed, for the same reason capture text
/// is: an edit may legitimately begin with a dash, and a parser that inspects
/// it first would either consume it or refuse the line.
/// `edit` and `append`: a name, then the text as words or `--stdin`, and for
/// `edit` an optional `--expect VERSION`.
///
/// Flags are read only before `--`; after it every word is text, so a line
/// that starts with a dash can still be written.
fn text_command(
    verb: &str,
    rest: &[String],
    takes_expect: bool,
) -> Result<(String, Source, Option<String>), Bad> {
    let Some((name, tail)) = rest.split_first() else {
        return Err(Bad::Value {
            command: verb.into(),
            problem: "no note name".into(),
        });
    };
    let mut expect = None;
    let mut stdin = false;
    let mut words: Vec<String> = Vec::new();
    let mut it = tail.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--" => {
                words.extend(it.by_ref().cloned());
                break;
            }
            "--stdin" if words.is_empty() => stdin = true,
            "--expect" if takes_expect => match it.next() {
                Some(v) => expect = Some(v.clone()),
                None => {
                    return Err(Bad::Value {
                        command: verb.into(),
                        problem: "--expect needs the version, as show --json gives it".into(),
                    });
                }
            },
            _ => words.push(a.clone()),
        }
    }
    let text = match (stdin, words.is_empty()) {
        (true, true) => Source::Stdin,
        (false, false) => Source::Words(words.join(" ")),
        (true, false) => {
            return Err(Bad::Value {
                command: verb.into(),
                problem: "text from --stdin or from the command line, not both".into(),
            });
        }
        (false, true) => {
            return Err(Bad::UnknownFlag {
                command: verb.into(),
                flag: format!("nothing to {verb}"),
            });
        }
    };
    Ok((name.clone(), text, expect))
}

fn name_and_rest(
    rest: &[String],
    verb: &str,
    build: impl Fn(String, &[String]) -> Command,
) -> Result<Command, Bad> {
    match rest {
        [] => Err(Bad::Value {
            command: verb.into(),
            problem: "no note name".into(),
        }),
        [_] => Err(Bad::UnknownFlag {
            command: verb.into(),
            flag: format!("nothing to {verb}"),
        }),
        [name, tail @ ..] => Ok(build(name.clone(), tail)),
    }
}

fn flagless(name: &str, rest: &[String], cmd: Command) -> Result<Command, Bad> {
    if let Some(flag) = rest.iter().find(|a| a.starts_with('-')) {
        return Err(Bad::UnknownFlag {
            command: name.to_string(),
            flag: flag.clone(),
        });
    }
    if let Some(extra) = rest.first() {
        return Err(Bad::UnknownFlag {
            command: name.to_string(),
            flag: extra.clone(),
        });
    }
    Ok(cmd)
}

/// The usage text.
pub const USAGE: &str = r#"capture — capture first, organise later

CAPTURE
  capture + <text>          store it, exactly as given
  capture + --stdin         store what arrives on standard input
  capture +                 read from a pipe, or prompt if there is a terminal
  capture + -- <text>       store text that starts with a dash

THE TERMINAL CHORD
  eval "$(capture shell-init zsh)"    in ~/.zshrc   (or bash, in ~/.bashrc)

  Then: type the text at your prompt and press Ctrl-B. The line is read
  before the shell parses it, so #, !, >, *, $VAR and backticks survive.
  The line clearing is the confirmation; if it stays, it was not saved.

READING
  capture timeline [n]      notes newest first, grouped by capture day
  capture show <name>       one note in full, with its links
  capture search <words>    full-text search, most relevant first
  capture links <name>      what it points at, and what points at it

  Any of these takes --json.

TIME AND TASKS
  capture agenda [day]           what is due and what occurs, default today
  capture cal [YYYY-MM]          a month, and what happens on each day
  capture cal --week [day]       the week containing a day, default this one
  capture tasks                  open tasks, newest first
  capture list <tag>             a list: its items, and what is left
  capture due                    what will speak, and when
  capture settle                 read captures nothing has read yet
  capture held                   events stored but not folded, and why
  capture redact --with-links <name>   destroy it and the notes linking to it
  capture redact --release <id>  let a destruction the ceiling held go ahead
  capture recover                copies kept before a redaction
  capture recover <id>           write one back as a new note
  capture skip <series> <day>    not this one; the rule is untouched
                               (unskip puts it back; <day> is 2026-10-12)
  capture move <series> <day> --to "thursday 10am"
                               this one happens at a different time
  capture move <series> <occ> <start-ms> [<end-ms>]
                               the same, in milliseconds
  capture amend <series> <occ> --title T [--location L] [--notes N]
                               this one only, with its own details
  capture tick <tag> <item>      tick one off; untick puts it back
  capture fix-list <note>        insert the colon a list was missing
  capture hour <note> am|pm      say which half of the clock "at 2" meant
  capture day <note> 1|2         say which of two days "next thursday" meant
  capture reread <note>          take back a correction; the text decides again
  capture page new <title> [--under <page> | --of @person]
                                 start a page: never read for dates, named by its title
  capture page <note>            make a capture a page
  capture unpage <note>          make a page a capture again
  capture tick-at <note> <n>     tick item n of one capture, counting from 1
                               (untick-at puts it back; use this where two
                                items read the same)
  capture mentions @sarah        every note naming a person, project or tag
                               (@person, ^project, #tag)
  capture people                 everyone the archive knows, most-said first
  capture projects               every project it knows
  capture entity @sarah          the page: names, backing note, and the notes
  capture entity-rename @sarah "Sarah Chen"    show a name of your choosing
  capture entity-merge @sarah-chen @sarah      say the two were one all along
  capture entity-bind ^acme <note>             the note that *is* the project
  capture entity-bind ^acme                    unbind it again
  capture forget <id>            abandon a plan, by its id from capture intents
  capture parent <page> <under>  put a page under another in the wiki
  capture parent <page>          put it back at the top
  capture task <name> [due]      say it is a task, optionally due YYYY-MM-DD
  capture done|undone <name>     finish a task, or open it again
  capture done|undone <name> <occ>   one instance of a series, by its start
  capture review                 a bounded session, five notes and then over
  capture engaged|deferred <name>    what became of one that was offered
  capture carries-forward <name>     missing this series leaves something owed
  capture read <text>            what the recognizer makes of it, storing nothing

  A capture becomes a task on its own only where a verb and a day agree —
  "call the dentist tomorrow". A verb alone is a suggestion and nothing more,
  which is what capture task is for.

  A list is made by capturing one:  #groceries: milk, eggs, bread
  One line, one capture, three items. Lists are tags, so anything else
  carrying #groceries joins the list without being filed into it.

CHANGING A NOTE
  capture edit <name> <text>     replace the text; the old version is kept
  capture edit <name> --stdin    the same, reading the text from standard input
  capture edit <name> --expect <version> ...
                               refused, writing nothing, if it has moved on
  capture append <name> <text>   add lines at the end (or --stdin); loses nothing
  capture tag <name> <tag>       add a tag
  capture untag <name> <tag>     retract it
  capture alias <name> [new]     give it a chosen name, or clear it
  capture pin|unpin <name>       keep it at the top, or stop
  capture mute <name>            stop it surfacing, without destroying it
  capture mute #<tag>            same, for everything carrying a tag
  capture mute @sarah            same, for everything naming a person
                               (or ^project; follows renames and merges)
  capture mute <from> <to>       same, for everything captured between two days
  capture mute <what> until <day>  until that day, then it comes back on its own
  capture unmute <what>          lift a mute; takes the same four forms as mute
  capture resolve <name>         dismiss a divergence; both versions are kept

DESTROYING SOMETHING
  capture redact <name>          destroy its content, after a cancellable window
  capture redact --cancel <id>   stop a queued redaction
  capture redact --cancel --all-since <t>   stop every redaction since then

  This is the only irreversible thing here. --force skips the prompt and
  nothing else: the window, the cap and the snapshot all remain.

OPERATING
  capture status            what the store holds, and anything needing attention
  capture log <name>        every event that touched a note
  capture quarantine        events set aside as unusable
  capture rejected          events that failed authentication
  capture rebuild           recompute the projection from the log
  capture verify            check every id, signature, body hash and chain
  capture doctor [--ask]    every check as a code to read aloud; --ask asks
                            whether the test notification appeared
  capture export            write everything appended since the last export
  capture restore <dir>     rebuild from exported events

  Pairing a second device is not a backup. It covers that device dying and
  nothing else: a bad event, a mistaken redaction or a broken projection is
  replicated everywhere within seconds, by design. Export is the only thing
  that covers those, and two devices in one bag is one accident.

  Export is incremental and takes no lock, so it is safe on a timer and safe
  while you are capturing. `install.sh --with-export` sets up a daily one.
  capture unflag <name>     clear a credential flag so the note is exported again
  capture pair              show a code, and admit whoever types it
  capture pair CODE --at ADDR
                          type a code shown on another device
  capture split NAME --from DATE --to RULE
                          change a repeating series from one occurrence on,
                          leaving what it was before it alone
  capture peers             every device in the roster, and when it was
                          last reached
  capture peers add HOST:PORT
                          try that address each sync round, for a network
                          where devices cannot find each other
  capture retire KEY        stop accepting what a device writes from now on
                          — for one that was lost or stolen
  capture readmit           get back in after every device was retired;
                          needs a second device to vouch, via capture cosign
  capture admission-seen KEY
                          say you know about a device another device admitted
  capture exports-pruned    say you have dealt with export files holding
                          destroyed content — redaction cannot reach them

  restore takes --ignore-redactions-after <time> to undo a mistaken redaction.
  It is not the default: an ordinary restore applies every redaction.
  capture set <key> [value] record a preference, or restore its default

AGENTS
  capture agent add <name>  enrol a program that writes as itself, not as you
  capture agent list        every agent, and whether it can still write
  capture agent instructions [--name <name>]
                          print a skill and a permission allowlist for it

  CAPTURE_AS=<name> before a command signs what it writes as that agent.
  Its notes stay out of the timeline and review, and say who wrote them.
  An agent's key is retired like a device's:  capture retire <name>
  Writes take --json: one object on stdout naming what was written.

COMMANDS
  capture init              create the store
  capture shell-init <sh>   print the snippet above (bash | zsh)
  capture help              this
  capture version           version

Everything after + is content. Anything else is an instruction, and an
instruction that is not recognised is refused rather than stored.
"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn p(args: &[&str]) -> Result<Command, Bad> {
        parse(args.iter().map(|s| s.to_string()))
    }

    fn words(args: &[&str]) -> String {
        match p(args) {
            Ok(Command::Capture(Source::Words(w))) => w,
            other => panic!("expected captured words, got {other:?}"),
        }
    }

    #[test]
    fn peers_add_takes_one_host_and_port() {
        assert!(matches!(
            p(&["peers", "add", "192.168.1.20:7878"]),
            Ok(Command::PeersAdd { addr }) if addr == "192.168.1.20:7878"
        ));
        assert!(matches!(
            p(&["peers", "add", "desk.local:7878"]),
            Ok(Command::PeersAdd { .. })
        ));
        for bad in [
            &["peers", "add"][..],
            &["peers", "add", "192.168.1.20"],
            &["peers", "add", "h:0"],
            &["peers", "add", "h:99999"],
            &["peers", "add", ":7878"],
            &["peers", "add", "a:1", "b:2"],
        ] {
            assert!(matches!(p(bad), Err(Bad::Value { .. })), "{bad:?}");
        }
        assert!(matches!(p(&["peers"]), Ok(Command::Peers { json: false })));
    }

    #[test]
    fn doctor_takes_only_ask() {
        assert!(matches!(p(&["doctor"]), Ok(Command::Doctor { ask: false })));
        assert!(matches!(
            p(&["doctor", "--ask"]),
            Ok(Command::Doctor { ask: true })
        ));
        assert!(p(&["doctor", "--frobnicate"]).is_err());
    }

    #[test]
    fn the_entity_commands_parse() {
        assert!(matches!(
            p(&["people"]),
            Ok(Command::People { json: false })
        ));
        assert!(matches!(
            p(&["people", "--json"]),
            Ok(Command::People { json: true })
        ));
        assert!(matches!(
            p(&["projects"]),
            Ok(Command::Projects { json: false })
        ));
        match p(&["entity", "@sarah"]) {
            Ok(Command::Entity { name, json: false }) => assert_eq!(name, "@sarah"),
            other => panic!("expected an entity page, got {other:?}"),
        }
        match p(&["entity-rename", "@sarah", "Sarah Chen"]) {
            Ok(Command::EntityRename { name, display }) => {
                assert_eq!(name, "@sarah");
                // One argument, not the rest of the line joined: a display name
                // with a space in it has to be quoted, which is what a shell is
                // for, and joining would swallow a second name silently.
                assert_eq!(display, "Sarah Chen");
            }
            other => panic!("expected a rename, got {other:?}"),
        }
        match p(&["entity-merge", "@a", "@b"]) {
            Ok(Command::EntityMerge { from, into }) => {
                assert_eq!((from.as_str(), into.as_str()), ("@a", "@b"));
            }
            other => panic!("expected a merge, got {other:?}"),
        }
        // Binding with a note, and unbinding without one. The absent argument is
        // the whole gesture, so it cannot be an error.
        match p(&["entity-bind", "^acme", "some-note"]) {
            Ok(Command::EntityBind { name, note }) => {
                assert_eq!(name, "^acme");
                assert_eq!(note.as_deref(), Some("some-note"));
            }
            other => panic!("expected a bind, got {other:?}"),
        }
        match p(&["entity-bind", "^acme"]) {
            Ok(Command::EntityBind { note: None, .. }) => {}
            other => panic!("expected an unbind, got {other:?}"),
        }
    }

    #[test]
    fn a_page_is_not_started_both_under_a_page_and_as_someones() {
        // G15. Either order, and the message says which choice to make.
        for args in [
            &["page", "new", "Trip", "--under", "travel", "--of", "@sarah"][..],
            &["page", "new", "Trip", "--of", "@sarah", "--under", "travel"][..],
        ] {
            match p(args) {
                Err(Bad::Value { problem, .. }) => {
                    assert!(problem.contains("not both"), "{problem}")
                }
                other => panic!("expected a refusal, got {other:?}"),
            }
        }
        assert!(matches!(
            p(&["page", "new", "Trip", "--under", "travel"]),
            Ok(Command::NewPage { .. })
        ));
        assert!(matches!(
            p(&["page", "new", "Trip", "--of", "@sarah"]),
            Ok(Command::NewPageFor { .. })
        ));
    }

    #[test]
    fn an_entity_command_with_the_wrong_arity_is_refused() {
        assert!(p(&["entity-rename", "@sarah"]).is_err());
        assert!(p(&["entity-merge", "@sarah"]).is_err());
        assert!(p(&["people", "extra"]).is_err());
    }

    #[test]
    fn missing_arguments_read_as_a_sentence() {
        // Thirteen sites reported "you gave me a flag I do not take" when they
        // meant "you did not give me enough", so the message came out as
        // "'show' does not take no note name" — and then offered to capture
        // the words "no note name", which is nonsense twice over.
        //
        // `Value` exists for exactly this and the difference is in the advice:
        // an unknown flag is often text somebody meant to capture, and a
        // missing argument is not.
        for (args, want) in [
            (vec!["show"], "capture show: no note name"),
            (vec!["search"], "capture search: no search terms"),
            (vec!["forget"], "capture forget: expected the id of a plan"),
        ] {
            let msg = p(&args).unwrap_err().message();
            assert!(msg.starts_with(want), "{args:?} still reads badly: {msg}");
            assert!(
                !msg.contains("does not take"),
                "{args:?} reports a missing argument as an unknown flag"
            );
        }
    }

    #[test]
    fn every_command_a_refusal_names_is_a_command() {
        // `admission-seen` used to say "give the key shown in capture health".
        // There is no `capture health` — it is `capture status` that shows it —
        // so the one message whose job was to point somewhere pointed nowhere,
        // and being told to run something that answers "is not a command" ends
        // the search in the wrong place.
        //
        // Over every source file in this binary, read from disk at run time
        // rather than named in a list.
        //
        // It used to scan this file alone, which is where the parser and the
        // usage text live — and refusals that name a command are written
        // wherever the refusing happens: `capture redact --release <id>` in
        // `ops.rs`, `capture recover <id>` in `write.rs`, `capture unflag
        // <name>` in `backup.rs`, `capture settle` in `read.rs`, `capture
        // readmit` in `pair.rs`. None of those was checked.
        //
        // Read rather than `include_str!`-ed because a list of file names is a
        // hand-kept list, and this test exists because a hand-kept list went
        // stale. Adding a module to this binary should not quietly narrow a
        // check.
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let sources: Vec<String> = std::fs::read_dir(&dir)
            .expect("the source directory is readable")
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "rs"))
            .map(|p| std::fs::read_to_string(&p).expect("a source file is readable"))
            .collect();
        assert!(
            sources.len() > 1,
            "the scan found one file or none, so it is back to where it started"
        );
        let mut checked = 0;
        for line in sources.iter().flat_map(|s| s.lines()) {
            // Messages only. Prose in a comment may say "capture text" meaning
            // the text of a capture, and does not promise a command exists.
            let trimmed = line.trim_start();
            if trimmed.starts_with("//") || !line.contains('"') {
                continue;
            }
            for (i, _) in line.match_indices("capture ") {
                let after = &line[i + "capture ".len()..];
                let word: String = after
                    .chars()
                    .take_while(|c| c.is_ascii_lowercase() || *c == '-')
                    .collect();
                // The binary's own name with no verb, and the help it prints.
                if word.is_empty() || word == "help" {
                    continue;
                }
                // Shown as a command, not mentioned as a noun. "expected a
                // capture and an item number" is about a captured note, and
                // promises no `capture and`. A command being shown ends the
                // string, ends the line, or is followed by a placeholder.
                let rest = &after[word.len()..];
                if !(rest.is_empty()
                    || rest.starts_with('"')
                    || rest.starts_with(" <")
                    || rest.starts_with("\\n"))
                {
                    continue;
                }
                checked += 1;
                // That the *verb* is known, not that this invocation is
                // complete. `capture intent` needs three arguments, and
                // demanding a parse would have made the test a second copy of
                // every command's arity — which is a thing to get wrong, not a
                // thing to assert.
                assert!(
                    !matches!(p(&[&word]), Err(Bad::UnknownCommand(_))),
                    "a message names `capture {word}`, which is not a command",
                );
            }
        }
        assert!(checked > 0, "the scan found nothing, so it proves nothing");
    }

    #[test]
    fn mute_tells_an_entity_from_a_tag_from_a_note() {
        // Four scopes behind one verb, and the sigil is what picks. `#` stays a
        // *tag* mute: both scopes exist, they are not the same thing, and
        // moving an existing command onto the other one silently would be the
        // worst kind of change.
        assert_eq!(
            p(&["mute", "@sarah"]).unwrap(),
            Command::Mute {
                what: MuteTarget::Entity("person".into(), "sarah".into()),
                until: None
            }
        );
        assert_eq!(
            p(&["mute", "^acme"]).unwrap(),
            Command::Mute {
                what: MuteTarget::Entity("project".into(), "acme".into()),
                until: None
            }
        );
        assert_eq!(
            p(&["mute", "#groceries"]).unwrap(),
            Command::Mute {
                what: MuteTarget::Tag("groceries".into()),
                until: None
            },
            "a tag is a tag mute, not an entity one"
        );
        assert_eq!(
            p(&["mute", "some-note"]).unwrap(),
            Command::Mute {
                what: MuteTarget::Note("some-note".into()),
                until: None
            }
        );

        // A sigil naming nothing is refused rather than muting an entity
        // called "", which would be every unnamed thing at once.
        for naming_nothing in [vec!["mute", "@"], vec!["mute", "^"]] {
            let msg = p(&naming_nothing).unwrap_err().message();
            assert!(msg.contains("names nothing"), "{naming_nothing:?}: {msg}");
        }
    }

    #[test]
    fn an_end_can_be_stated_on_any_of_the_four_scopes() {
        // Taken off the end before the target is parsed, so the four forms
        // keep working unchanged and none of them knows about it. Each is
        // checked rather than one, because "it worked for a note" is how the
        // other three come to be forgotten.
        assert_eq!(
            p(&["mute", "some-note", "until", "2026-12-25"]).unwrap(),
            Command::Mute {
                what: MuteTarget::Note("some-note".into()),
                until: Some("2026-12-25".into())
            }
        );
        assert_eq!(
            p(&["mute", "#groceries", "until", "2026-12-25"]).unwrap(),
            Command::Mute {
                what: MuteTarget::Tag("groceries".into()),
                until: Some("2026-12-25".into())
            }
        );
        assert_eq!(
            p(&["mute", "@sarah", "until", "2026-12-25"]).unwrap(),
            Command::Mute {
                what: MuteTarget::Entity("person".into(), "sarah".into()),
                until: Some("2026-12-25".into())
            }
        );
        // The two-day form still reaches the date range, with the end after
        // it. Three dates in one command, and the first two mean something
        // different from the third.
        assert_eq!(
            p(&["mute", "2026-01-01", "2026-01-31", "until", "2026-12-25"]).unwrap(),
            Command::Mute {
                what: MuteTarget::Days("2026-01-01".into(), "2026-01-31".into()),
                until: Some("2026-12-25".into())
            }
        );

        // Shape is checked here; whether the day has already passed is not,
        // because that needs a clock and this layer has none.
        let msg = p(&["mute", "some-note", "until", "christmas"])
            .unwrap_err()
            .message();
        assert!(msg.contains("YYYY-MM-DD"), "{msg}");

        let msg = p(&["mute", "some-note", "until"]).unwrap_err().message();
        assert!(msg.contains("after 'until'"), "{msg}");
    }

    #[test]
    fn unmute_takes_the_same_four_forms_as_mute() {
        // One verb's worth of parsing shared with `mute`, so a target form
        // cannot be accepted by one and not the other.
        assert_eq!(
            p(&["unmute", "some-note"]).unwrap(),
            Command::Unmute {
                what: MuteTarget::Note("some-note".into())
            }
        );
        assert_eq!(
            p(&["unmute", "#groceries"]).unwrap(),
            Command::Unmute {
                what: MuteTarget::Tag("groceries".into())
            }
        );
        assert_eq!(
            p(&["unmute", "@sarah"]).unwrap(),
            Command::Unmute {
                what: MuteTarget::Entity("person".into(), "sarah".into())
            }
        );
        assert_eq!(
            p(&["unmute", "2026-01-01", "2026-01-31"]).unwrap(),
            Command::Unmute {
                what: MuteTarget::Days("2026-01-01".into(), "2026-01-31".into())
            }
        );

        // An end on a lift is a contradiction, not a no-op. Accepting it
        // silently would leave somebody believing they had rescheduled a mute.
        let msg = p(&["unmute", "some-note", "until", "2026-12-25"])
            .unwrap_err()
            .message();
        assert!(msg.contains("takes no 'until'"), "{msg}");
    }

    #[test]
    fn skip_names_an_occurrence_the_way_done_does() {
        assert_eq!(
            p(&["skip", "standup", "1791208800000"]).unwrap(),
            Command::Skip {
                name: "standup".into(),
                occ: OccRef::Ms(1_791_208_800_000),
                skipped: true,
            }
        );
        assert_eq!(
            p(&["unskip", "standup", "1791208800000"]).unwrap(),
            Command::Skip {
                name: "standup".into(),
                occ: OccRef::Ms(1_791_208_800_000),
                skipped: false,
            }
        );
        // A series with no occurrence is the `done` shape, which means a task.
        // There is no task to skip, so it is a refusal rather than a guess.
        assert!(p(&["skip", "standup"]).is_err());
    }

    #[test]
    fn move_and_amend_name_an_occurrence_the_same_way() {
        // One convention for naming an occurrence across all four exceptions,
        // rather than a second one for the two that arrived later.
        assert_eq!(
            p(&["move", "standup", "1791208800000", "1791216000000"]).unwrap(),
            Command::Move {
                name: "standup".into(),
                occ: OccRef::Ms(1_791_208_800_000),
                to: MoveTo::Ms {
                    start_ms: 1_791_216_000_000,
                    end_ms: None,
                },
            }
        );
        assert_eq!(
            p(&[
                "move",
                "standup",
                "1791208800000",
                "1791216000000",
                "1791219600000"
            ])
            .unwrap(),
            Command::Move {
                name: "standup".into(),
                occ: OccRef::Ms(1_791_208_800_000),
                to: MoveTo::Ms {
                    start_ms: 1_791_216_000_000,
                    end_ms: Some(1_791_219_600_000),
                },
            }
        );
        assert_eq!(
            p(&[
                "amend",
                "standup",
                "1791208800000",
                "--title",
                "in the annexe"
            ])
            .unwrap(),
            Command::Amend {
                name: "standup".into(),
                occ: OccRef::Ms(1_791_208_800_000),
                title: Some("in the annexe".into()),
                location: None,
                notes: None,
            }
        );

        // A move with nowhere to move to, and an amendment that amends
        // nothing: both are requests that meant something and said nothing, so
        // both are refused rather than written as events that fold to nothing.
        assert!(p(&["move", "standup", "1791208800000"]).is_err());
        assert!(p(&["amend", "standup", "1791208800000"]).is_err());

        let msg = p(&["amend", "standup", "1791208800000"])
            .unwrap_err()
            .message();
        assert!(
            msg.contains("--title"),
            "the refusal should say what was missing: {msg}"
        );
    }

    /// "this Monday's gym" is a date, and moving it is a sentence.
    #[test]
    fn an_occurrence_is_named_by_its_date_and_moved_by_text() {
        assert_eq!(
            p(&["move", "gym", "2026-10-12", "--to", "thursday 10am"]).unwrap(),
            Command::Move {
                name: "gym".into(),
                occ: OccRef::Date("2026-10-12".into()),
                to: MoveTo::Text("thursday 10am".into()),
            }
        );
        assert_eq!(
            p(&["skip", "gym", "2026-10-12"]).unwrap(),
            Command::Skip {
                name: "gym".into(),
                occ: OccRef::Date("2026-10-12".into()),
                skipped: true,
            }
        );
        assert_eq!(
            p(&["done", "gym", "2026-10-12"]).unwrap(),
            Command::Occurrence {
                name: "gym".into(),
                occ: OccRef::Date("2026-10-12".into()),
                done: true,
            }
        );
        assert!(p(&["skip", "gym", "monday"]).is_err(), "a date, not a word");
    }

    #[test]
    fn recover_takes_an_optional_id() {
        assert_eq!(
            p(&["recover"]).unwrap(),
            Command::Recover {
                id: None,
                json: false
            }
        );
        assert_eq!(
            p(&["recover", "a4f4d4ac"]).unwrap(),
            Command::Recover {
                id: Some("a4f4d4ac".into()),
                json: false
            }
        );
        assert_eq!(
            p(&["recover", "--json"]).unwrap(),
            Command::Recover {
                id: None,
                json: true
            }
        );
    }

    #[test]
    fn held_takes_nothing_but_json() {
        assert!(matches!(
            p(&["held"]).unwrap(),
            Command::Held { json: false }
        ));
        assert!(matches!(
            p(&["held", "--json"]).unwrap(),
            Command::Held { json: true }
        ));
    }

    #[test]
    fn settle_takes_nothing_but_json() {
        assert!(matches!(
            p(&["settle"]).unwrap(),
            Command::Settle { json: false }
        ));
        assert!(matches!(
            p(&["settle", "--json"]).unwrap(),
            Command::Settle { json: true }
        ));
        let msg = p(&["settle", "now"]).unwrap_err().message();
        assert!(msg.contains("does not take"), "{msg}");
    }

    #[test]
    fn a_flag_that_really_is_unknown_still_offers_capture() {
        // The other half, so the fix above did not simply delete a good
        // message. A word this command does not know is often something
        // somebody meant to write down.
        let msg = p(&["tasks", "--wat"]).unwrap_err().message();
        assert!(msg.contains("does not take --wat"), "{msg}");
        assert!(msg.contains("capture + --wat"), "{msg}");
    }

    #[test]
    fn a_bare_word_is_refused_rather_than_captured() {
        // The rule the sigil exists to make possible. A tool that stored what
        // it did not recognise would turn every typo into a note.
        assert_eq!(p(&["buy", "milk"]), Err(Bad::UnknownCommand("buy".into())));
    }

    #[test]
    fn a_mistyped_command_is_refused_rather_than_captured() {
        // The case that makes the rule worth its cost: this is what silent
        // capture would have quietly filed away as a note reading "stauts".
        assert_eq!(p(&["stauts"]), Err(Bad::UnknownCommand("stauts".into())));
    }

    #[test]
    fn the_refusal_shows_how_to_capture_it_instead() {
        // Being told "no" is not useful on its own. Someone who typed this was
        // probably trying to write something down.
        let m = p(&["buy", "milk"]).unwrap_err().message();
        assert!(m.contains("capture + buy"), "got: {m}");
    }

    #[test]
    fn nothing_at_all_is_refused() {
        assert_eq!(p(&[]), Err(Bad::Empty));
        assert!(Bad::Empty.message().contains("capture +"));
    }

    #[test]
    fn words_after_the_sigil_are_joined() {
        assert_eq!(words(&["+", "buy", "milk"]), "buy milk");
    }

    #[test]
    fn a_capture_may_look_exactly_like_a_flag() {
        // The reason this file does not use an argument parser. Every one of
        // these is ordinary text to someone writing a note.
        assert_eq!(words(&["+", "-n", "5", "left"]), "-n 5 left");
        assert_eq!(words(&["+", "--verbose", "logging"]), "--verbose logging");
        assert_eq!(words(&["+", "-h"]), "-h");
    }

    #[test]
    fn a_capture_may_look_exactly_like_a_command() {
        assert_eq!(words(&["+", "status", "meeting"]), "status meeting");
        assert_eq!(words(&["+", "init"]), "init");
    }

    #[test]
    fn stdin_is_a_flag_only_when_it_is_the_whole_request() {
        assert_eq!(p(&["+", "--stdin"]), Ok(Command::Capture(Source::Stdin)));
        // With anything after it, it is text — a note *about* the flag.
        assert_eq!(
            words(&["+", "--stdin", "is", "a", "flag"]),
            "--stdin is a flag"
        );
    }

    #[test]
    fn the_double_dash_makes_a_flag_into_text() {
        // The escape hatch that makes recognising `--stdin` safe: there is
        // always a way to capture it literally.
        assert_eq!(words(&["+", "--", "--stdin"]), "--stdin");
        assert_eq!(words(&["+", "--", "-h"]), "-h");
    }

    #[test]
    fn a_bare_sigil_leaves_the_source_to_the_process() {
        // Whether this reads a pipe or prompts depends on whether a terminal is
        // attached, which is not a question about the arguments.
        assert_eq!(p(&["+"]), Ok(Command::Capture(Source::Unspecified)));
    }

    #[test]
    fn commands_that_take_nothing_say_so() {
        assert_eq!(
            p(&["status", "--json"]),
            Err(Bad::UnknownFlag {
                command: "status".into(),
                flag: "--json".into()
            })
        );
    }

    #[test]
    fn a_rejected_flag_is_told_how_to_become_text() {
        let m = p(&["status", "--nonsense"]).unwrap_err().message();
        assert!(m.contains("capture + --nonsense"), "got: {m}");
    }

    #[test]
    fn help_and_version_answer_to_their_usual_flags() {
        for f in ["help", "--help", "-h"] {
            assert_eq!(p(&[f]), Ok(Command::Help));
        }
        for f in ["version", "--version", "-V"] {
            assert_eq!(p(&[f]), Ok(Command::Version));
        }
    }

    /// `capture pair --help` opened a real pairing window on a real archive,
    /// because `pair` read anything that was not its code as nothing at all.
    /// A request for help must never be a request to do the thing.
    #[test]
    fn help_after_a_command_is_help_not_the_command() {
        for cmd in ["pair", "retire", "export", "restore", "split", "status"] {
            for f in ["--help", "-h"] {
                assert_eq!(p(&[cmd, f]), Ok(Command::Help), "{cmd} {f}");
            }
        }
        assert_eq!(p(&["pair", "123456", "--help"]), Ok(Command::Help));
    }

    /// Except where the words are the note's.
    #[test]
    fn help_inside_a_capture_is_text() {
        assert_eq!(
            p(&["+", "--", "-h"]),
            Ok(Command::Capture(source(&["-h".to_string()])))
        );
    }

    #[test]
    fn pair_refuses_a_flag_it_does_not_know() {
        assert!(p(&["pair", "--hepl"]).is_err());
        assert_eq!(
            p(&["pair", "123456", "--at", "10.0.0.2:41823"]),
            Ok(Command::Pair {
                code: Some("123456".into()),
                at: Some("10.0.0.2:41823".into())
            })
        );
        assert_eq!(
            p(&["pair"]),
            Ok(Command::Pair {
                code: None,
                at: None
            })
        );
    }

    #[test]
    fn hour_takes_a_note_and_a_half() {
        assert_eq!(
            p(&["hour", "gym", "pm"]),
            Ok(Command::Hour {
                name: "gym".into(),
                pm: true
            })
        );
        assert_eq!(
            p(&["hour", "gym", "am"]),
            Ok(Command::Hour {
                name: "gym".into(),
                pm: false
            })
        );
        assert!(p(&["hour", "gym"]).is_err());
        assert!(p(&["hour", "gym", "14"]).is_err());
    }

    #[test]
    fn cal_takes_a_month_or_a_week() {
        assert_eq!(
            p(&["cal"]),
            Ok(Command::Cal {
                span: CalSpan::Month(None),
                json: false
            })
        );
        assert_eq!(
            p(&["cal", "2026-11", "--json"]),
            Ok(Command::Cal {
                span: CalSpan::Month(Some("2026-11".into())),
                json: true
            })
        );
        assert_eq!(
            p(&["cal", "--week", "2026-10-15"]),
            Ok(Command::Cal {
                span: CalSpan::Week(Some("2026-10-15".into())),
                json: false
            })
        );
        assert!(p(&["cal", "--month"]).is_err());
    }

    #[test]
    fn usage_shows_the_capture_form_first() {
        // Capture is the thing this tool is for. A usage screen that lists it
        // fourth teaches that it is one feature among several.
        let capture = USAGE
            .find("capture + <text>")
            .expect("capture form is listed");
        let init = USAGE.find("capture init").expect("init is listed");
        assert!(capture < init);
    }
}
