# Tasks

## 1. The harness

- [ ] 1.0 A file of its own, `tests/confirmation.rs`. `scenarios.rs` states in
      its header that prose must not be asserted, and that rule is right — but
      here the prose *is* the contract, because §1.1 makes what the prompt says
      a requirement. Two files with opposite rules and a stated reason beats one
      quietly contradicting its own preamble.
- [ ] 1.1 A helper that runs a command with stdin supplied, spawning with
      `Stdio::piped()` rather than calling `.output()`.
- [ ] 1.2 Confirm it works at all before writing anything that depends on it:
      a redaction answered with the wrong name should come back without having
      destroyed anything.

## 2. The exact name

- [ ] 2.1 A near miss destroys nothing.
- [ ] 2.2 An empty line destroys nothing.
- [ ] 2.3 The name with trailing text destroys nothing — different from the
      trailing newline a terminal supplies, which is forgiven on purpose.
- [ ] 2.4 The exact name orders the destruction.
- [ ] 2.5 Each case checks the **store**, not the output: the text is still
      there and nothing is queued. A prompt that printed "Nothing destroyed"
      and queued it anyway is the failure worth catching.

## 3. What the prompt says

- [ ] 3.1 A note with a recording: the prompt says the recording goes too.
- [ ] 3.2 A note without one: no mention of a recording.
- [ ] 3.3 `--with-links`: every note that will go is named, not counted.

## 4. Guards

- [ ] 4.1 Verify each test is non-vacuous against the behaviour it describes,
      not just against a typo.
- [ ] 4.2 `make ci`.
- [ ] 4.3 Check whether the pty suite already covers any of this, so this does
      not become a second place the same thing is asserted differently.
