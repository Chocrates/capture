//! `capture doctor` can be read aloud from a work machine (it-runs-on-a-mac D6).
//!
//! Every line is a fixed code, a result and a fixed phrase. Nothing the system
//! or the person supplied may appear: note text, paths, user or host names,
//! addresses, keys. Seeded with sentinels and checked for every one of them.

use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

const NOTE: &str = "zqsentinelnote";
const DIR: &str = "zqsentineldir";

struct Store(PathBuf);

impl Store {
    fn new(name: &str) -> Store {
        let d = std::env::temp_dir()
            .join(format!("{DIR}-{name}-{}", std::process::id()))
            .join("archive");
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let s = Store(d);
        assert!(s.run(&["init"], None).status.success());
        s
    }

    fn run(&self, args: &[&str], stdin: Option<&str>) -> Output {
        let mut c = Command::new(env!("CARGO_BIN_EXE_capture"));
        c.args(args)
            .env("CAPTURE_HOME", &self.0)
            .env("CAPTURE_SILENT", "1")
            .env("XDG_RUNTIME_DIR", self.0.parent().unwrap())
            .env("XDG_CONFIG_HOME", self.0.parent().unwrap().join("config"))
            .env("TZ", "UTC")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = c.spawn().unwrap();
        if let Some(text) = stdin {
            use std::io::Write;
            child
                .stdin
                .take()
                .unwrap()
                .write_all(text.as_bytes())
                .unwrap();
        } else {
            drop(child.stdin.take());
        }
        child.wait_with_output().unwrap()
    }
}

impl Drop for Store {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(self.0.parent().unwrap());
    }
}

fn host() -> Option<String> {
    std::fs::read_to_string("/etc/hostname")
        .ok()
        .map(|h| h.trim().to_string())
        .filter(|h| h.len() >= 3)
}

#[test]
fn nothing_personal_reaches_the_output() {
    let s = Store::new("sentinels");
    for text in [
        format!("{NOTE} budget review"),
        format!("{NOTE} 192.168.77.31:7878 call back"),
        format!("{NOTE} every day at 9am"),
    ] {
        assert!(s.run(&["+", &text], None).status.success());
    }
    let key = String::from_utf8_lossy(&s.run(&["status"], None).stdout).to_string();
    let out = s.run(&["doctor"], None);
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    let all = format!("{stdout}{stderr}");

    let mut needles: Vec<String> = vec![
        NOTE.into(),
        DIR.into(),
        "192.168.77.31".into(),
        "7878".into(),
        s.0.to_string_lossy().into(),
        std::env::temp_dir().to_string_lossy().into(),
    ];
    for var in ["USER", "LOGNAME", "HOME"] {
        if let Ok(v) = std::env::var(var)
            && v.len() >= 3
        {
            needles.push(v);
        }
    }
    needles.extend(host());
    // Device keys as `status` prints them: any 8+ hex run in status that
    // looks like a key must not be in doctor's output.
    for word in key.split(|c: char| !c.is_ascii_hexdigit()) {
        if word.len() >= 8 {
            needles.push(word.to_string());
        }
    }
    for n in &needles {
        assert!(!all.contains(n.as_str()), "doctor printed {n:?}:\n{all}");
    }

    // And every line is the fixed shape.
    let lines: Vec<&str> = stdout.lines().collect();
    assert!(lines.len() >= 15, "{stdout}");
    for l in &lines {
        let (code, rest) = l.split_once(' ').expect(l);
        assert!(
            code.len() == 2
                && code.as_bytes()[0].is_ascii_uppercase()
                && code.as_bytes()[1].is_ascii_digit(),
            "{l}"
        );
        let (result, reason) = rest.split_once(": ").expect(l);
        assert!(["pass", "fail", "skip"].contains(&result), "{l}");
        assert!(
            reason
                .chars()
                .all(|c| c.is_ascii_lowercase() || " -,()0123456789".contains(c)),
            "a reason that is not a fixed phrase: {l}"
        );
    }
}

#[test]
fn a_healthy_store_passes_the_store_checks() {
    let s = Store::new("healthy");
    let out = String::from_utf8_lossy(&s.run(&["doctor"], None).stdout).to_string();
    for code in ["B1 pass", "B2 pass", "B3 pass"] {
        assert!(out.contains(code), "{code} missing:\n{out}");
    }
    // Silenced, so no notification is sent and none is claimed.
    assert!(out.contains("N2 skip"), "{out}");
    // Not asked, so nobody is asked whether they saw it.
    assert!(out.contains("N3 skip"), "{out}");
}

#[test]
fn ask_puts_the_question_and_records_the_answer() {
    let s = Store::new("ask");
    let out = s.run(&["doctor", "--ask"], Some("n\n"));
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    // Silenced here, so nothing was sent and there is nothing to have seen.
    assert!(stdout.contains("N3 skip"), "{stdout}");
    assert!(s.run(&["doctor", "--frobnicate"], None).status.code() != Some(0));
}

#[test]
fn a_held_lock_fails_b2() {
    let s = Store::new("held");
    let _held = capture_store::lock::Writer::acquire(&s.0.join("writer.lock"), "test")
        .unwrap()
        .expect("the lock was free");
    let out = String::from_utf8_lossy(&s.run(&["doctor"], None).stdout).to_string();
    assert!(out.contains("B2 fail: held by another process"), "{out}");
}

#[test]
fn every_code_is_documented() {
    let doc = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../docs/doctor-codes.md"
    ))
    .expect("docs/doctor-codes.md");
    let s = Store::new("documented");
    let out = String::from_utf8_lossy(&s.run(&["doctor"], None).stdout).to_string();
    for l in out.lines() {
        let code = &l[..2];
        assert!(
            doc.contains(&format!("| {code} ")),
            "{code} is not in the doc"
        );
        let reason = l.split_once(": ").unwrap().1;
        assert!(doc.contains(reason), "{reason:?} is not in the doc");
    }
}

#[test]
fn a_damaged_log_fails_b3() {
    let s = Store::new("damaged");
    assert!(s.run(&["+", "something to damage"], None).status.success());
    let ok = Command::new("sqlite3")
        .arg(s.0.join("events.db"))
        .arg("UPDATE events SET body = X'DEADBEEF' WHERE kind = 1;")
        .status()
        .expect("sqlite3 is available");
    assert!(ok.success());
    let out = s.run(&["doctor"], None);
    assert!(
        out.status.success(),
        "a failing check is an answer, not an error"
    );
    let out = String::from_utf8_lossy(&out.stdout).to_string();
    assert!(
        out.contains("B3 fail: damaged events, run capture verify"),
        "{out}"
    );
}
