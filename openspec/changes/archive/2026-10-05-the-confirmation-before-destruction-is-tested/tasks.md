# Tasks

## 1. The harness

- [x] 1.0 A file of its own, `tests/confirmation.rs`. `scenarios.rs` states in
      its header that prose must not be asserted, and that rule is right — but
      here the prose *is* the contract, because §1.1 makes what the prompt says
      a requirement. Two files with opposite rules and a stated reason beats one
      quietly contradicting its own preamble.
- [x] 1.1 A helper that runs a command with stdin supplied, spawning with
      `Stdio::piped()` rather than calling `.output()`.
- [x] 1.2 Confirm it works at all before writing anything that depends on it:
      a redaction answered with the wrong name should come back without having
      destroyed anything.

## 2. The exact name

- [x] 2.1 A near miss destroys nothing.
- [x] 2.2 An empty line destroys nothing.
- [x] 2.3 The name with trailing text destroys nothing — different from the
      trailing newline a terminal supplies, which is forgiven on purpose.
- [x] 2.4 The exact name orders the destruction.
- [x] 2.5 Each case checks the **store**, not the output: the text is still
      there and nothing is queued. A prompt that printed "Nothing destroyed"
      and queued it anyway is the failure worth catching.

## 3. What the prompt says

- [ ] 3.1 A note with a recording: the prompt says the recording goes too.
      **Not done, and not doable from here.** The command line cannot attach a
      recording — they come from the phone's voice capture — so the positive
      case is unreachable without giving `capture-cli` a dev-dependency on
      `capture-store` to build the capture directly, which would let these
      tests reach past the binary they exist to drive. The negative half is
      covered and the test is named for what it actually checks. The clause's
      *behaviour* is covered by `blob_redaction.rs`; its wording is covered by
      nothing, and that clause exists because redaction once destroyed the text
      and left the recording.
- [x] 3.2 A note without one: no mention of a recording.
- [x] 3.3 `--with-links`: every note that will go is named, not counted.

## 4. Guards

- [x] 4.1 Verify each test is non-vacuous against the behaviour it describes,
      not just against a typo. Sabotaged the comparison both ways. Accepting
      any answer fails the three refusals and the `--with-links` case; refusing
      every answer fails `the_exact_name_orders_the_destruction`, which exists
      precisely so the refusals cannot pass for the wrong reason.
- [x] 4.2 `make ci`. 79 suites green — the new file is the 79th; `deny` fails on
      network only.
- [x] 4.3 Check whether the pty suite already covers any of this, so this does
      not become a second place the same thing is asserted differently. It does
      not — `redact` appears nowhere in `pty.rs`.
