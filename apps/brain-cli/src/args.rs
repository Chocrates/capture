//! Turning a command line into an intention.
//!
//! # Why this is not a derive macro
//!
//! Everything after `+` is capture text, including things that look exactly
//! like flags. `brain + -n 5 items left` is a note about five items, and any
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
//! `brain buy milk` is refused, and it is refused *loudly*, with the capture
//! form spelled out. Treating unknown input as text would be friendlier for
//! exactly one keystroke and wrong forever after: `brain stauts` becomes a note
//! reading "stauts", `brain serach cats` becomes a note reading "serach cats",
//! and the archive slowly fills with typo-shaped debris that nothing marks as
//! accidental.
//!
//! The sigil is what makes the boundary legible. `+` means "this is content";
//! everything else means "this is an instruction", and an instruction that is
//! not understood is an error rather than a guess.

/// What the user asked for.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Command {
    /// Capture. Carries the text source rather than the text, because reading
    /// from a terminal is a side effect and this module performs none.
    Capture(Source),
    /// Create a store.
    Init,
    /// Report on the store.
    Status,
    /// Print the shell snippet that installs the capture chord.
    ShellInit(String),
    /// Notes, newest first, grouped by capture day.
    Timeline { limit: Option<usize>, json: bool },
    /// One note in full.
    Show { name: String, json: bool },
    /// Notes matching a query.
    Search { query: String, json: bool },
    /// What a note points at, and what points at it.
    Links { name: String, json: bool },
    /// Replace a note's text with a new version.
    Edit { name: String, text: String },
    /// Add a tag.
    Tag { name: String, tag: String },
    /// Retract a tag.
    Untag { name: String, tag: String },
    /// Assign or clear a chosen name.
    Alias { name: String, alias: Option<String> },
    /// Pin or unpin.
    Pin { name: String, pinned: bool },
    /// Suppress something from surfacing.
    Mute { what: MuteTarget },
    /// Stop every redaction issued since a moment.
    RedactCancelSince { since: u64 },
    /// Dismiss a divergence.
    Resolve { name: String },
    /// Every event that touched a note.
    Log { name: String, json: bool },
    /// Events set aside as unusable.
    Quarantine { json: bool },
    /// Events that failed authentication.
    Rejected { json: bool },
    /// Recompute the projection from the log.
    Rebuild,
    /// Record a preference, or restore its default.
    Set { key: String, value: Option<String> },
    /// Check every id, signature, body hash and origin chain.
    Verify { json: bool },
    /// Acknowledge that stale export files were dealt with by hand.
    ExportsPruned,
    /// Write everything appended since the last export.
    Export { json: bool },
    /// Rebuild a store from exported events.
    Restore {
        from: String,
        ignore_redactions_after: Option<u64>,
    },
    /// Clear a note's credential flag so it is exported again.
    Unflag { name: String },
    /// Destroy a note's content, after a cancellable window.
    Redact { name: String, force: bool },
    /// Stop a queued redaction.
    RedactCancel { id: String },
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
                 brain + {w} ...\n\
                 \n\
                 For the list of commands: brain help"
            ),
            Bad::UnknownFlag { command, flag } => format!(
                "'{command}' does not take {flag}.\n\
                 \n\
                 If you meant to capture that text, put a + first — everything \
                 after + is taken literally:\n    \
                 brain + {flag}\n\
                 \n\
                 For the list of commands: brain help"
            ),
            Bad::Value { command, problem } => format!(
                "brain {command}: {problem}.\n\n\
                 For how to use it: brain help"
            ),
            Bad::Empty => "Nothing to do.\n\
                 \n\
                 To capture a note:\n    \
                 brain + your note here\n\
                 \n\
                 For the list of commands: brain help"
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

    match first.as_str() {
        "+" => Ok(Command::Capture(source(&args[1..]))),
        "init" => flagless("init", &args[1..], Command::Init),
        "status" => flagless("status", &args[1..], Command::Status),
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
                return Err(Bad::UnknownFlag {
                    command: "search".into(),
                    flag: "no search terms".into(),
                });
            }
            Ok(Command::Search {
                query: terms.join(" "),
                json,
            })
        }
        "edit" => name_and_rest(&args[1..], "edit", |name, rest| Command::Edit {
            name,
            text: rest.join(" "),
        }),
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
                _ => Err(Bad::UnknownFlag {
                    command: "alias".into(),
                    flag: "expected a note and an optional new name".into(),
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
        "mute" => {
            let rest = &args[1..];
            match rest {
                // A tag, recognised by the sigil it is written with everywhere
                // else. Anything else is a note's name.
                [one] if one.starts_with('#') => Ok(Command::Mute {
                    what: MuteTarget::Tag(one.trim_start_matches('#').to_string()),
                }),
                [one] => Ok(Command::Mute {
                    what: MuteTarget::Note(one.clone()),
                }),
                [from, to] if looks_like_a_day(from) && looks_like_a_day(to) => Ok(Command::Mute {
                    what: MuteTarget::Days(from.clone(), to.clone()),
                }),
                _ => Err(Bad::Value {
                    command: "mute".into(),
                    problem: "expected a note, a #tag, or two dates as YYYY-MM-DD".into(),
                }),
            }
        }
        "resolve" => one_name(&args[1..], "resolve", |name, _| Command::Resolve { name }),
        "log" => one_name(&args[1..], "log", |name, json| Command::Log { name, json }),
        "quarantine" => Ok(Command::Quarantine {
            json: has_json(&args[1..]),
        }),
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
            let names: Vec<&String> = rest.iter().filter(|a| !a.starts_with("--")).collect();
            match names.as_slice() {
                [name] => Ok(Command::Redact {
                    name: (*name).clone(),
                    force,
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
        "exports-pruned" => flagless("exports-pruned", &args[1..], Command::ExportsPruned),
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
            _ => Err(Bad::UnknownFlag {
                command: "set".into(),
                flag: "expected a setting and an optional value".into(),
            }),
        },
        "shell-init" => match args.get(1) {
            Some(shell) => Ok(Command::ShellInit(shell.clone())),
            None => Err(Bad::UnknownFlag {
                command: "shell-init".into(),
                flag: "no shell (try: bash, zsh)".into(),
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
        // in first position. Later on it is text: `brain + note the --stdin
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
/// typed as `brain + costs   $40 #urgent` arrives having already lost the
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

fn has_json(rest: &[String]) -> bool {
    rest.iter().any(|a| a == "--json")
}

/// Commands shaped `brain <verb> <name> [--json]`.
fn one_name(
    rest: &[String],
    verb: &str,
    build: impl Fn(String, bool) -> Command,
) -> Result<Command, Bad> {
    let json = has_json(rest);
    let names: Vec<&String> = rest.iter().filter(|a| *a != "--json").collect();
    match names.as_slice() {
        [name] => Ok(build((*name).clone(), json)),
        [] => Err(Bad::UnknownFlag {
            command: verb.into(),
            flag: "no note name".into(),
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
fn name_and_rest(
    rest: &[String],
    verb: &str,
    build: impl Fn(String, &[String]) -> Command,
) -> Result<Command, Bad> {
    match rest {
        [] => Err(Bad::UnknownFlag {
            command: verb.into(),
            flag: "no note name".into(),
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
pub const USAGE: &str = r#"brain — capture first, organise later

CAPTURE
  brain + <text>          store it, exactly as given
  brain + --stdin         store what arrives on standard input
  brain +                 read from a pipe, or prompt if there is a terminal
  brain + -- <text>       store text that starts with a dash

THE TERMINAL CHORD
  eval "$(brain shell-init zsh)"    in ~/.zshrc   (or bash, in ~/.bashrc)

  Then: type the text at your prompt and press Ctrl-B. The line is read
  before the shell parses it, so #, !, >, *, $VAR and backticks survive.
  The line clearing is the confirmation; if it stays, it was not saved.

READING
  brain timeline [n]      notes newest first, grouped by capture day
  brain show <name>       one note in full, with its links
  brain search <words>    full-text search, most relevant first
  brain links <name>      what it points at, and what points at it

  Any of these takes --json.

CHANGING A NOTE
  brain edit <name> <text>     replace the text; the old version is kept
  brain tag <name> <tag>       add a tag
  brain untag <name> <tag>     retract it
  brain alias <name> [new]     give it a chosen name, or clear it
  brain pin|unpin <name>       keep it at the top, or stop
  brain mute <name>            stop it surfacing, without destroying it
  brain mute #<tag>            same, for everything carrying a tag
  brain mute <from> <to>       same, for everything captured between two days
  brain resolve <name>         dismiss a divergence; both versions are kept

DESTROYING SOMETHING
  brain redact <name>          destroy its content, after a cancellable window
  brain redact --cancel <id>   stop a queued redaction
  brain redact --cancel --all-since <t>   stop every redaction since then

  This is the only irreversible thing here. --force skips the prompt and
  nothing else: the window, the cap and the snapshot all remain.

OPERATING
  brain status            what the store holds, and anything needing attention
  brain log <name>        every event that touched a note
  brain quarantine        events set aside as unusable
  brain rejected          events that failed authentication
  brain rebuild           recompute the projection from the log
  brain verify            check every id, signature, body hash and chain
  brain export            write everything appended since the last export
  brain restore <dir>     rebuild from exported events
  brain unflag <name>     clear a credential flag so the note is exported again
  brain exports-pruned    say you have dealt with export files holding
                          destroyed content — redaction cannot reach them

  restore takes --ignore-redactions-after <time> to undo a mistaken redaction.
  It is not the default: an ordinary restore applies every redaction.
  brain set <key> [value] record a preference, or restore its default

COMMANDS
  brain init              create the store
  brain shell-init <sh>   print the snippet above (bash | zsh)
  brain help              this
  brain version           version

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
        assert!(m.contains("brain + buy"), "got: {m}");
    }

    #[test]
    fn nothing_at_all_is_refused() {
        assert_eq!(p(&[]), Err(Bad::Empty));
        assert!(Bad::Empty.message().contains("brain +"));
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
        assert!(m.contains("brain + --nonsense"), "got: {m}");
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

    #[test]
    fn usage_shows_the_capture_form_first() {
        // Capture is the thing this tool is for. A usage screen that lists it
        // fourth teaches that it is one feature among several.
        let capture = USAGE
            .find("brain + <text>")
            .expect("capture form is listed");
        let init = USAGE.find("brain init").expect("init is listed");
        assert!(capture < init);
    }
}
