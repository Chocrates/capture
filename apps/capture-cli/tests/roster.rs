//! The roster verbs, on two real archives that have really been paired.
//!
//! # Why these went untested
//!
//! `pair`, `retire --yes`, `cosign`, `readmit` and `admission-seen` all need a
//! second device — one with its own key, whose events this one either accepts
//! or refuses. A single store cannot stand in for that, and each of these was
//! left with a note saying so.
//!
//! # How there are two devices here
//!
//! Two stores, each its own `CAPTURE_HOME`, driven through the binary. Pairing
//! is the real command on both ends over the real pairing port, because the
//! exchange is the thing under test. Sync is `round::reconcile` called in this
//! process over a loopback socket — the same function the daemon and the phone
//! run — because the daemon binds a fixed port and two of them on one machine
//! would fight over it, which would be testing the harness.
//!
//! # One at a time
//!
//! The pairing port is fixed, so these run with `--test-threads=1`. A test
//! that cannot bind it fails at the bind, saying so.

use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};

use capture_store::paths::StorePaths;
use capture_sync::session::Session as Secure;
use capture_sync::tcp::Tcp;

struct Device {
    home: PathBuf,
}

impl Device {
    fn new(name: &str) -> Device {
        let home =
            std::env::temp_dir().join(format!("capture-roster-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(&home).unwrap();
        let d = Device { home };
        d.ok(&["init"]);
        d
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut c = Command::new(env!("CARGO_BIN_EXE_capture"));
        c.args(args)
            .env("CAPTURE_HOME", &self.home)
            .env("TZ", "UTC");
        c
    }

    fn run(&self, args: &[&str]) -> (i32, String) {
        let out = self.command(args).output().expect("capture runs");
        let mut text = String::from_utf8_lossy(&out.stdout).to_string();
        text.push_str(&String::from_utf8_lossy(&out.stderr));
        (out.status.code().unwrap_or(-1), text)
    }

    fn ok(&self, args: &[&str]) -> String {
        let (code, out) = self.run(args);
        assert!(code == 0, "capture {args:?} exited {code}: {out}");
        out
    }

    fn capture(&self, text: &str) -> String {
        self.ok(&["+", text]).trim().to_string()
    }

    fn paths(&self) -> StorePaths {
        StorePaths::under(&self.home)
    }

    fn identity(&self) -> capture_store::Identity {
        capture_store::Identity::load(&self.paths()).unwrap()
    }

    /// This device's key, as `peers` and `retire` name it.
    fn key(&self) -> String {
        capture_core::id::short_key(&self.identity().public().to_bytes())
    }

    /// Every key in this device's roster, from `peers --json`.
    fn roster(&self) -> Vec<String> {
        let v: serde_json::Value = serde_json::from_str(&self.ok(&["peers", "--json"])).unwrap();
        v["peers"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["key"].as_str().unwrap().to_string())
            .collect()
    }

    /// Whether a note with this text is in the archive.
    fn holds(&self, text: &str) -> bool {
        let v: serde_json::Value =
            serde_json::from_str(&self.ok(&["timeline", "1000", "--json"])).unwrap();
        v.as_array()
            .unwrap()
            .iter()
            .any(|n| n["text"].as_str() == Some(text))
    }
}

impl Drop for Device {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.home).ok();
    }
}

/// `capture pair` on `offerer`, and the code it shows typed into `joiner`.
///
/// Returns what each side said.
fn pair(offerer: &Device, joiner: &Device) -> (String, String) {
    let mut offer: Child = offerer
        .command(&["pair"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the offer starts");

    // Read until the code appears, rather than sleeping a guessed amount.
    let mut stdout = offer.stdout.take().unwrap();
    let mut shown = String::new();
    let code = loop {
        use std::io::Read;
        let mut buf = [0u8; 256];
        let n = stdout.read(&mut buf).unwrap();
        if n == 0 {
            let mut err = String::new();
            offer.stderr.take().unwrap().read_to_string(&mut err).ok();
            panic!("the offer ended without showing a code:\n{shown}{err}");
        }
        shown.push_str(&String::from_utf8_lossy(&buf[..n]));
        if let Some(code) = shown
            .lines()
            .find_map(|l| l.strip_prefix("  code   "))
            .map(str::trim)
            .filter(|c| c.len() == 6)
        {
            break code.to_string();
        }
    };

    let joined = joiner.ok(&["pair", &code, "--at", "127.0.0.1"]);
    {
        use std::io::Read;
        stdout.read_to_string(&mut shown).unwrap();
    }
    let status = offer.wait().unwrap();
    assert!(status.success(), "the offer failed:\n{shown}");
    (shown, joined)
}

/// One round between two devices, as the daemon runs it.
fn sync(a: &Device, b: &Device) {
    let now = jiff::Timestamp::now().as_millisecond() as u64;
    let a_id = a.identity();
    let b_id = b.identity();

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();

    let b_paths = b.paths();
    let b_secret = b_id.noise_secret();
    let responder = std::thread::spawn(move || {
        let stream = listener.accept().unwrap().0;
        let tcp = Tcp::new(stream).unwrap();
        let mut secure = Secure::accept(tcp, &b_secret).expect("the responder handshook");
        capture_store::round::reconcile(&b_paths, &mut secure, now, false, &|| false)
            .expect("the responder finished")
    });

    let tcp = Tcp::new(TcpStream::connect(addr).unwrap()).unwrap();
    let mut secure =
        Secure::initiate(tcp, &a_id.noise_secret(), &b_id.noise_public()).expect("handshook");
    capture_store::round::reconcile(&a.paths(), &mut secure, now, true, &|| false)
        .expect("the initiator finished");
    responder.join().expect("no panic");
}

/// The events a device's log holds, by count, as `status` reports them.
fn events(d: &Device) -> u64 {
    d.ok(&["status"])
        .lines()
        .find_map(|l| l.strip_prefix("events"))
        .and_then(|n| n.trim().parse().ok())
        .expect("status reports an event count")
}

/// What a device's `quarantine --json` set aside, by reason.
fn set_aside(d: &Device) -> Vec<String> {
    let v: serde_json::Value = serde_json::from_str(&d.ok(&["quarantine", "--json"])).unwrap();
    v.as_array()
        .unwrap()
        .iter()
        .map(|r| r["reason"].as_str().unwrap().to_string())
        .collect()
}

/// Two archives become one roster, and each holds what the other wrote.
///
/// The pairing exchange end to end — the code shown, typed, the grant carried
/// back — which nothing had run through the command line before.
#[test]
#[ignore = "binds the pairing port; run with --ignored --test-threads=1"]
fn pairing_makes_two_archives_one_roster() {
    let a = Device::new("pair-a");
    let b = Device::new("pair-b");
    let (offered, joined) = pair(&a, &b);

    assert!(
        offered.contains(&format!("Paired with {}", b.key())),
        "the offerer should name who it admitted: {offered}"
    );
    assert!(
        joined.contains(&format!("{} vouched for this device", a.key())),
        "{joined}"
    );
    // The joiner is the second device, and says so. This said "1 device(s)":
    // the grant carries the admitter's roster from before the admission.
    assert!(joined.contains("2 device(s) in the roster"), "{joined}");

    // The offerer knows the joiner at once, because it wrote the admission.
    let mut both = vec![a.key(), b.key()];
    both.sort();
    assert_eq!(a.roster(), both);

    a.capture("written on a");
    b.capture("written on b");
    sync(&b, &a);

    assert_eq!(b.roster(), both, "the joiner learned the roster by syncing");
    assert!(a.holds("written on b"), "a never received b's note");
    assert!(b.holds("written on a"), "b never received a's note");
}

/// A retired device's later writes are refused; its earlier ones stand.
#[test]
#[ignore = "binds the pairing port; run with --ignored --test-threads=1"]
fn retiring_a_device_refuses_only_what_it_writes_afterwards() {
    let a = Device::new("retire-a");
    let b = Device::new("retire-b");
    pair(&a, &b);
    b.capture("before retirement");
    sync(&b, &a);

    // Without --yes it says what would happen and writes nothing.
    let before = events(&a);
    let asked = a.ok(&["retire", &b.key()]);
    assert!(asked.contains("Re-run with --yes"), "{asked}");
    assert_eq!(events(&a), before, "a retirement was written without --yes");
    assert_eq!(a.roster().len(), 2);

    let said = a.ok(&["retire", &b.key(), "--yes"]);
    assert!(said.contains(&format!("Retired {}", b.key())), "{said}");
    assert_eq!(a.roster(), vec![a.key()]);

    b.capture("after retirement");
    sync(&b, &a);

    // §5.3: past events remain valid, later ones are refused — set aside,
    // visibly, and not destroyed.
    assert!(a.holds("before retirement"), "a past event was discarded");
    assert!(
        !a.holds("after retirement"),
        "a retired device's write was accepted"
    );
    assert_eq!(set_aside(&a), vec!["post_retire".to_string()]);

    // And b, which did not issue its own retirement, does not apply it to
    // itself (§5.3's first lockout guard). It still lists itself — which it
    // did not, before `peers` honoured that rule as the gate already did.
    assert!(
        b.roster().contains(&b.key()),
        "a device dropped itself on another's say-so: {:?}",
        b.roster()
    );
}

/// A member vouches for a retired device, and it is a member again.
#[test]
#[ignore = "binds the pairing port; run with --ignored --test-threads=1"]
fn a_member_can_readmit_a_retired_device() {
    let a = Device::new("readmit-a");
    let b = Device::new("readmit-b");
    pair(&a, &b);
    sync(&b, &a);
    a.ok(&["retire", &b.key(), "--yes"]);
    b.capture("while retired");
    sync(&b, &a);

    // Asked with nothing in hand, the retired device says what to run where.
    let asked = b.ok(&["readmit"]);
    let blob = asked
        .lines()
        .find_map(|l| l.trim().strip_prefix("capture cosign "))
        .unwrap_or_else(|| panic!("no cosign line: {asked}"))
        .to_string();

    // A device cannot vouch for itself: that is one say-so written twice. The
    // command's own wording is asserted, not just the failure — the store
    // refuses a self-signed re-admission too, in nearly the same words, so
    // anything looser passes with the command's guard removed.
    let before = events(&b);
    let (code, said) = b.run(&["cosign", &blob]);
    assert_ne!(code, 0, "a device vouched for itself: {said}");
    assert!(said.contains("say-so written twice"), "{said}");
    assert_eq!(events(&b), before, "a refused self-vouch wrote something");

    let said = a.ok(&["cosign", &blob]);
    assert!(said.contains(&format!("Re-admitted {}", b.key())), "{said}");
    sync(&b, &a);
    assert_eq!(a.roster().len(), 2, "the re-admission did not take");

    b.capture("after readmission");
    sync(&b, &a);
    assert!(
        a.holds("after readmission"),
        "a re-admitted device is still refused"
    );

    // What it wrote while retired stays set aside. §5.3: re-admission is not
    // a pardon; a retirement reissued with a later head is what lets those in.
    assert!(!a.holds("while retired"));
    assert_eq!(set_aside(&a), vec!["post_retire".to_string()]);
}

/// The co-signature path, where the voucher is not a member either.
///
/// It cannot write the re-admission, so it prints the half it can sign and the
/// other device writes it. The signature is checked before anything is
/// written, so a mangled one is refused.
#[test]
#[ignore = "binds the pairing port; run with --ignored --test-threads=1"]
fn a_retired_voucher_hands_over_a_co_signature() {
    let a = Device::new("cosign-a");
    let b = Device::new("cosign-b");
    pair(&a, &b);
    sync(&b, &a);

    // a retires itself, so in its own view it is no longer a member.
    a.ok(&["retire", &a.key(), "--yes"]);
    let blob = b
        .ok(&["readmit"])
        .lines()
        .find_map(|l| l.trim().strip_prefix("capture cosign "))
        .unwrap()
        .to_string();

    let said = a.ok(&["cosign", &blob]);
    assert!(said.contains("cannot write the re-admission"), "{said}");
    let line = said
        .lines()
        .find_map(|l| l.trim().strip_prefix("capture readmit "))
        .unwrap_or_else(|| panic!("no readmit line: {said}"));
    let words: Vec<&str> = line.split_whitespace().collect();
    let (sig, by) = (words[1], words[3]);
    assert_eq!((words[0], words[2]), ("--cosign", "--by"), "{line}");

    // Mangled: one hex digit changed. Refused, and nothing written.
    let mut bad = sig.to_string();
    let flip = if bad.ends_with('0') { "1" } else { "0" };
    bad.replace_range(bad.len() - 1.., flip);
    let before = events(&b);
    let (code, refused) = b.run(&["readmit", "--cosign", &bad, "--by", by]);
    assert_ne!(code, 0, "a bad co-signature was accepted: {refused}");
    assert!(refused.contains("does not vouch"), "{refused}");
    assert_eq!(events(&b), before, "a refused re-admission wrote something");

    let said = b.ok(&["readmit", "--cosign", sig, "--by", by]);
    assert!(said.contains(&format!("Re-admitted {}", b.key())), "{said}");
    assert_eq!(events(&b), before + 1, "the re-admission was not written");
}

/// A device admitted elsewhere is reported until somebody says they saw it.
///
/// §5.3: any member may admit a device. One this device did not admit can read
/// everything, so it is said once, loudly, and stays said until acknowledged.
#[test]
#[ignore = "binds the pairing port; run with --ignored --test-threads=1"]
fn an_admission_made_elsewhere_is_reported_until_acknowledged() {
    let a = Device::new("seen-a");
    let b = Device::new("seen-b");
    let c = Device::new("seen-c");
    pair(&a, &b);
    sync(&b, &a);

    // Nothing to report about b: a admitted it.
    assert!(!a.ok(&["status"]).contains("did not pair or admit"));

    pair(&b, &c);
    sync(&b, &a);

    let status = a.ok(&["status"]);
    assert!(
        status.contains("did not pair or admit") && status.contains(&c.key()),
        "an admission a did not make went unreported: {status}"
    );

    // A key that names nothing is said to, and acknowledges nothing.
    assert!(
        a.ok(&["admission-seen", "ffffffff"])
            .contains("No unacknowledged")
    );
    assert!(a.ok(&["status"]).contains(&c.key()));

    let said = a.ok(&["admission-seen", &c.key()]);
    assert!(
        said.contains(&format!("Acknowledged {}", c.key())),
        "{said}"
    );
    assert!(
        !a.ok(&["status"]).contains("did not pair or admit"),
        "an acknowledged admission is still reported"
    );
    assert!(
        a.ok(&["admission-seen", &c.key()])
            .contains("No unacknowledged")
    );
}

/// Events from an origin nobody vouched for are refused, and counted.
#[test]
#[ignore = "binds the pairing port; run with --ignored --test-threads=1"]
fn a_strangers_events_are_refused_and_counted() {
    let a = Device::new("stranger-a");
    let d = Device::new("stranger-d");
    d.capture("from a stranger");
    d.ok(&["export"]);

    assert!(a.ok(&["rejected"]).contains("No event has failed"));
    a.ok(&["restore", d.home.join("export").to_str().unwrap()]);

    assert!(
        !a.holds("from a stranger"),
        "an unvouched-for note was accepted"
    );
    let v: serde_json::Value = serde_json::from_str(&a.ok(&["rejected", "--json"])).unwrap();
    let unknown = v
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["reason"] == "UnknownOrigin")
        .unwrap_or_else(|| panic!("nothing refused as unknown: {v}"));
    assert!(unknown["count"].as_u64().unwrap() >= 1, "{v}");
    assert!(a.ok(&["rejected"]).contains("UnknownOrigin"));
}
