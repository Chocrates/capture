# the-confirmation-before-destruction-is-tested

## Why

Redaction is the only irreversible operation in the system. The thing standing
in front of it is a prompt that names what will go and asks for the note's name
to be typed back.

**Nothing tests it.** Every `redact` in the suite passes `--force`, which exists
precisely to skip the prompt, and nothing in the workspace pipes stdin to the
binary at all — the only `Stdio` use anywhere is the pty test's controlling
terminal setup. So the guard in front of the one operation that cannot be undone
is the least exercised code in the repo.

Four scenarios describe it and none is reached:

- a redaction with a recording says the recording goes too;
- one without says only what there is;
- `--with-links` names every note that will go, not just the one asked for;
- declining destroys nothing.

The third and fourth are the ones that matter most. §1.1 calls this "the one
confirmation in the system that must not be trained away", and the code's own
comment explains the risk it is guarding: "a flag that quietly widens what a
familiar prompt covers is exactly how it would be. Somebody who has typed the
name once has learned that the prompt means one note."

The recording clause has history: `blobs::destroy` was written, tested and
called by nothing, so redaction destroyed the text and left the audio. The line
in the prompt that says the recording goes is the user-visible half of that fix,
and it is unasserted.

## What Changes

- The scenario harness can give a command stdin. It is one method and nothing
  else in the suite needed it, which is why it was never written.
- Tests for the four scenarios: the wording when a recording is attached and
  when none is, the per-note listing under `--with-links`, and that declining
  leaves everything intact.
- A test that the prompt refuses anything but the exact name — a near miss, an
  empty line, the right name with trailing text — because "type the name"
  carries no weight if a stray newline passes.
- No change to the prompt itself. This is a change about what is checked.

## Capabilities

### Modified Capabilities

- `cli` — the confirmation is exercised rather than described.

## Non-goals

- **No change to `--force`.** It suppresses the prompt and nothing else: the
  cap, the window and the snapshot all remain. A flag that turned off the safety
  rails too would be the flag everyone reaches for.
- **No change to the wording.** Where a test and the prompt disagree the prompt
  is right, unless the spec says otherwise — these tests are being written after
  the fact and must not quietly become the specification.
- **No pty.** The prompt reads a line from stdin; it does not need a terminal,
  and the pty suite exists for the things that do.
