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
    /// Print usage.
    Help,
    /// Print the version.
    Version,
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
pub const USAGE: &str = "\
brain — capture first, organise later

CAPTURE
  brain + <text>          store it, exactly as given
  brain + --stdin         store what arrives on standard input
  brain +                 read from a pipe, or prompt if there is a terminal
  brain + -- <text>       store text that starts with a dash

COMMANDS
  brain init              create the store
  brain status            what the store holds, and where
  brain help              this
  brain version           version

Everything after + is content. Anything else is an instruction, and an
instruction that is not recognised is refused rather than stored.
";

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
