//! Driving a real interactive shell through a pseudoterminal.
//!
//! # Why a pty, and not a simpler test
//!
//! The capture chord's whole claim is that it reads the line *before* the shell
//! parses it. Checking that claim requires a shell that is actually parsing
//! lines, which means an interactive one, which means a terminal.
//!
//! Every cheaper approach passes while the real thing stays broken:
//!
//! - **`bash -c '...'`** — non-interactive. Readline never loads, `bind -x`
//!   does nothing, and history expansion of `!` and `^` is *off by default in
//!   non-interactive shells*. A test here shows `!` surviving because nothing
//!   tried to expand it, and reports success for the one metacharacter most
//!   likely to destroy a line in real use.
//! - **Piping a script into `bash -i`** — the shell reads stdin as a script
//!   rather than from a terminal, so there is still no line editor and still no
//!   buffer for a binding to read.
//! - **Calling the shell function directly** — the exact mistake the design
//!   documents. `READLINE_LINE` is set only while a binding is running; called
//!   as a command it is unset, and the capture silently stores nothing.
//!
//! So: a pty, `bash -i` and `zsh -i` on the far end of it, text typed in a byte
//! at a time, and a `Ctrl-B` at the end. What comes back out of the store has to
//! match what went in, byte for byte.
//!
//! # What is being defended
//!
//! A note is captured verbatim or it is not captured. Shell metacharacters are
//! the common way for that to fail quietly:
//!
//! | typed | what a command-based capture stores |
//! |---|---|
//! | `fix auth #client-acme` | `fix auth` — the tag became a comment |
//! | `revenue > costs` | `revenue` — the rest went into a file named `costs` |
//! | `try !important again` | history expansion, or an error |
//! | `` deploy `date` `` | the command **executed** |
//!
//! None of these announce themselves. The note looks fine until the day it is
//! needed.

// This is a test harness: it forks, execs and drives a terminal. The clock ban
// exists to keep time out of the fold, and nothing here goes near the fold.
#![allow(clippy::disallowed_methods, clippy::disallowed_types)]

use std::io::{Read, Write};
use std::os::fd::{AsFd, OwnedFd};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// Ctrl-B, the default chord.
const CHORD: u8 = 0x02;

/// How long to wait for a shell to finish starting, or a capture to land.
const PATIENCE: Duration = Duration::from_secs(10);

fn capture() -> &'static str {
    env!("CARGO_BIN_EXE_capture")
}

// ------------------------------------------------------------------ pty ----

/// A pseudoterminal with a shell on the far end.
struct Pty {
    master: OwnedFd,
    child: Child,
    /// What the shell printed before the first key, for a failure message.
    startup: String,
}

impl Pty {
    /// Start a shell attached to a new terminal.
    fn spawn(shell: &str, args: &[&str], env: &[(&str, &str)]) -> std::io::Result<Pty> {
        use rustix::pty::{OpenptFlags, grantpt, openpt, ptsname, unlockpt};

        let master = openpt(OpenptFlags::RDWR | OpenptFlags::NOCTTY)?;
        grantpt(&master)?;
        unlockpt(&master)?;
        let name = ptsname(&master, Vec::new())?;
        let slave_path = PathBuf::from(String::from_utf8_lossy(name.as_bytes()).to_string());

        let slave = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&slave_path)?;
        let err = slave.try_clone()?;
        let out = slave.try_clone()?;

        let mut cmd = Command::new(shell);
        cmd.args(args)
            .stdin(Stdio::from(slave))
            .stdout(Stdio::from(out))
            .stderr(Stdio::from(err))
            // A real terminal type. `dumb` is tempting for a clean transcript
            // and wrong: zsh's line editor checks the terminal's capabilities
            // and behaves differently without them, so the test would exercise
            // a path no person is on.
            .env("TERM", "xterm")
            .env("PS1", "$ ")
            .env("PROMPT", "$ ");
        for (k, v) in env {
            cmd.env(k, v);
        }

        // The child must lead its own session and own this terminal, or the
        // shell finds no controlling tty, disables job control, and in zsh's
        // case declines to run its line editor at all.
        unsafe {
            cmd.pre_exec(|| {
                rustix::process::setsid()?;
                let stdin = std::io::stdin();
                rustix::process::ioctl_tiocsctty(stdin.as_fd())?;
                Ok(())
            });
        }

        let child = cmd.spawn()?;
        Ok(Pty {
            master,
            child,
            startup: String::new(),
        })
    }

    /// Type bytes, exactly as given.
    ///
    /// One byte at a time with a pause, because a line editor reading a burst
    /// may treat it as a paste and apply bracketed-paste handling — which is
    /// its own code path, and not the one a person typing exercises.
    fn type_bytes(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        let mut f = std::fs::File::from(self.master.try_clone()?);
        for b in bytes {
            f.write_all(&[*b])?;
            f.flush()?;
            std::thread::sleep(Duration::from_millis(2));
        }
        Ok(())
    }

    /// Wait until the shell has drawn its prompt, then drain what it echoed.
    ///
    /// A fixed pause was here, and on a slow CI runner zsh was still starting
    /// when the first key arrived: its line editor flushes typeahead as it
    /// starts, so "fix auth" was captured as "ix auth". The prompt is drawn by
    /// the line editor itself, so once it is on the screen keys reach it.
    fn wait_for_prompt(&mut self) {
        let seen = self.read_until(b"$ ", 500);
        self.startup = String::from_utf8_lossy(&seen).escape_debug().to_string();
        // A moment for the editor to finish drawing what follows the prompt.
        std::thread::sleep(Duration::from_millis(100));
        self.drain();
    }

    /// Type the first key until the line editor echoes it.
    ///
    /// Waiting for the prompt was not enough on a CI runner: zsh still
    /// dropped the first key now and then. What is under test is how the
    /// finished line reaches the store, not keys typed while a shell starts,
    /// so the first key is confirmed before the rest follow.
    fn type_first_until_echoed(&mut self, first: u8) -> std::io::Result<()> {
        for _ in 0..5 {
            self.type_bytes(&[first])?;
            if self.read_until(&[first], 100).contains(&first) {
                return Ok(());
            }
        }
        Err(std::io::Error::other(
            "the shell never echoed the first key",
        ))
    }

    /// Read what the shell prints until `want` appears, for up to `steps`
    /// 20 ms steps. Returns everything read.
    fn read_until(&mut self, want: &[u8], steps: u32) -> Vec<u8> {
        let mut f = std::fs::File::from(self.master.try_clone().unwrap());
        let raw = rustix::fs::fcntl_getfl(f.as_fd()).unwrap();
        rustix::fs::fcntl_setfl(f.as_fd(), raw | rustix::fs::OFlags::NONBLOCK).ok();
        let mut seen = Vec::new();
        let mut buf = [0u8; 4096];
        for _ in 0..steps {
            while let Ok(n) = f.read(&mut buf) {
                if n == 0 {
                    break;
                }
                seen.extend_from_slice(&buf[..n]);
            }
            if seen.windows(want.len()).any(|w| w == want) {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        rustix::fs::fcntl_setfl(f.as_fd(), raw).ok();
        seen
    }

    /// Drain whatever the shell has echoed, so it does not confuse a later read.
    fn drain(&mut self) {
        let mut f = std::fs::File::from(self.master.try_clone().unwrap());
        let raw = rustix::fs::fcntl_getfl(f.as_fd()).unwrap();
        rustix::fs::fcntl_setfl(f.as_fd(), raw | rustix::fs::OFlags::NONBLOCK).ok();
        let mut buf = [0u8; 4096];
        while let Ok(n) = f.read(&mut buf) {
            if n == 0 {
                break;
            }
        }
        rustix::fs::fcntl_setfl(f.as_fd(), raw).ok();
    }
}

impl Drop for Pty {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

// ---------------------------------------------------------------- store ----

/// A scratch store, removed when the test ends.
struct Store(PathBuf);

impl Store {
    fn new(name: &str) -> Store {
        let d = std::env::temp_dir().join(format!("capture-pty-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();

        let ok = Command::new(capture())
            .arg("init")
            .env("CAPTURE_HOME", &d)
            .output()
            .expect("capture init runs");
        assert!(ok.status.success(), "capture init: {ok:?}");
        Store(d)
    }

    fn path(&self) -> &Path {
        &self.0
    }

    /// Every note's text, as the store has it.
    ///
    /// Read straight out of the log rather than through a read command, so the
    /// check does not wait on `timeline` or `show` existing, and so nothing
    /// between the capture and the assertion can normalise the text.
    fn texts(&self) -> Vec<String> {
        read_capture_texts(&self.0)
    }

    /// Wait for the store to hold `n` captures, or give up.
    fn wait_for(&self, n: usize) -> Vec<String> {
        let start = Instant::now();
        loop {
            let t = self.texts();
            if t.len() >= n {
                return t;
            }
            if start.elapsed() > PATIENCE {
                panic!("timed out waiting for {n} capture(s); store holds {t:?}");
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

impl Drop for Store {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).ok();
    }
}

/// Pull capture text directly out of the log.
fn read_capture_texts(root: &Path) -> Vec<String> {
    use capture_core::{body::CaptureBody, event::Kind};
    let Ok(log) = capture_store::log::Log::open_read(&root.join("events.db")) else {
        return Vec::new();
    };
    let Ok(events) = log.all() else {
        return Vec::new();
    };
    events
        .into_iter()
        .filter(|e| e.header.kind == Kind::CAPTURE)
        .filter_map(|e| {
            minicbor::decode::<CaptureBody>(e.body.as_deref()?)
                .ok()
                .map(|b| b.text)
        })
        .collect()
}

// ----------------------------------------------------------------- runs ----

/// Type a line into a live shell, press the chord, and return what was stored.
fn capture_through(shell: &str, store: &Store, line: &str) -> Vec<String> {
    let init = Command::new(capture())
        .args(["shell-init", shell])
        .output()
        .expect("shell-init runs");
    assert!(init.status.success());
    let snippet = String::from_utf8(init.stdout).unwrap();

    // The snippet goes into a startup file and is sourced. Passing it on the
    // command line would mean the shell parsing it as an argument, which is the
    // thing under test.
    //
    // Each shell needs its startup file in the place it actually looks:
    //
    //   bash  --rcfile <path>, with --noprofile so nothing else interferes
    //   zsh   $ZDOTDIR/.zshrc — and *not* `-f`, which would skip rc files
    //         entirely and leave the widget undefined. That mistake produces a
    //         shell where the chord does nothing, which looks exactly like the
    //         binding being broken. `-d` does skip the *global* rc files,
    //         the zsh equivalent of bash's --noprofile: a CI runner's
    //         /etc/zsh/zshrc ran compinit, which stopped to ask "[y] or [n]?"
    //         about insecure directories and took the first typed key as the
    //         answer.
    let rc = match shell {
        "bash" => store.path().join("bashrc"),
        "zsh" => store.path().join(".zshrc"),
        _ => unreachable!(),
    };
    std::fs::write(
        &rc,
        format!("PATH={}:$PATH\n{snippet}\n", capture_dir().display()),
    )
    .unwrap();

    let rc_str = rc.to_str().unwrap().to_string();
    let args: Vec<&str> = match shell {
        "bash" => vec!["--noprofile", "--rcfile", &rc_str, "-i"],
        "zsh" => vec!["-d", "-i"],
        _ => unreachable!(),
    };

    let mut pty = Pty::spawn(
        shell,
        &args,
        &[
            ("CAPTURE_HOME", store.path().to_str().unwrap()),
            ("ZDOTDIR", store.path().to_str().unwrap()),
        ],
    )
    .expect("a shell starts on the terminal");

    pty.wait_for_prompt();

    let (first, rest) = line.as_bytes().split_first().expect("a line to type");
    pty.type_first_until_echoed(*first)
        .unwrap_or_else(|e| panic!("{e}; the shell printed: {}", pty.startup));
    pty.type_bytes(rest).expect("the line is typed");
    std::thread::sleep(Duration::from_millis(150));
    pty.type_bytes(&[CHORD]).expect("the chord is pressed");

    let stored = store.wait_for(1);
    if stored.first().map(String::as_str) != Some(line) {
        eprintln!("{shell}: at startup the shell printed: {}", pty.startup);
    }
    stored
}

fn capture_dir() -> PathBuf {
    PathBuf::from(capture()).parent().unwrap().to_path_buf()
}

// ---------------------------------------------------------------- tests ----

/// Every character that quietly destroys a line when a shell sees it first.
///
/// Each is a real thing to write down, and each fails differently: `#` truncates
/// silently, `>` creates a file, `!` may abort the line, `` ` `` executes.
const HOSTILE: &[(&str, &str)] = &[
    ("hash", "fix auth #client-acme"),
    ("redirect", "revenue > costs this quarter"),
    ("bang", "try !important again"),
    ("tilde", "check ~/notes for the draft"),
    ("caret", "the ^ character in regexes"),
    ("glob", "delete *.tmp files someday"),
    ("dollar", "it costs $HOME plus $40"),
    ("backtick", "run `date` before deploying"),
    ("pipe", "grep foo | sort is the idiom"),
    ("semicolon", "first this; then that"),
    ("ampersand", "tom & jerry, backgrounded &"),
    ("quotes", "she said \"no\" and 'meant' it"),
    ("parens", "a note (with parens) inside"),
    ("brace", "expand {a,b} in the shell"),
    ("backslash", "the path C:\\temp\\new"),
];

fn check_shell(shell: &str) {
    for (name, line) in HOSTILE {
        let store = Store::new(&format!("{shell}-{name}"));
        let stored = capture_through(shell, &store, line);
        assert_eq!(
            stored.len(),
            1,
            "{shell}/{name}: expected exactly one capture, got {stored:?}"
        );
        assert_eq!(
            stored[0], *line,
            "{shell}/{name}: the shell altered the line before it was captured"
        );
    }
}

#[test]
#[ignore = "drives a real terminal; run with --ignored"]
fn bash_captures_the_line_exactly_as_typed() {
    check_shell("bash");
}

#[test]
#[ignore = "drives a real terminal; run with --ignored"]
fn zsh_captures_the_line_exactly_as_typed() {
    check_shell("zsh");
}

#[test]
#[ignore = "drives a real terminal; run with --ignored"]
fn a_command_based_capture_would_have_failed() {
    // The control. Without this, the tests above prove only that *something*
    // works — not that the keybinding was necessary. Here the same lines go
    // through the shell as arguments, the way a `b <text>` command would
    // receive them, and the damage is asserted rather than hoped for.
    let store = Store::new("control");
    let rc = store.path().join("rc");
    std::fs::write(&rc, "").unwrap();

    let out = Command::new("bash")
        .args([
            "--noprofile",
            "--norc",
            "-i",
            "-c",
            "capture + fix auth #client-acme",
        ])
        .env("CAPTURE_HOME", store.path())
        .env("PATH", format!("{}:/usr/bin:/bin", capture_dir().display()))
        .output()
        .expect("bash runs");
    assert!(out.status.success(), "the command itself succeeds: {out:?}");

    let stored = store.wait_for(1);
    assert_eq!(
        stored[0], "fix auth",
        "the shell should have eaten the tag as a comment — if this ever \
         stores the whole line, the keybinding may no longer be necessary and \
         this test should be revisited rather than deleted"
    );
}
