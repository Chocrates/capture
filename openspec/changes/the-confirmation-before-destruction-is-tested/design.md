# Design

## Why it was never tested, which is also how to test it

`Store::run` builds a `Command`, sets `CAPTURE_HOME`, and calls `.output()`.
That inherits or nulls stdin, so a command that reads a line gets nothing. Every
`redact` in the suite therefore passes `--force`, and once that is the habit
there is no pressure to make the harness able to answer a prompt.

So the harness gains one method: spawn with `Stdio::piped()`, write the answer,
read the output. It is small, and the fact that it had to be written is the
finding — the guard in front of the only irreversible operation was the least
exercised code in the repo, for want of a method nothing else needed.

## A separate file, because `scenarios.rs` forbids this on purpose

That file's header is explicit: "Asserted against `--json`, not against prose.
Human-readable output is meant to change — wording gets clearer, columns get
wider. A test that asserted on it would either break constantly or, worse, be
written loosely enough not to break and stop checking anything."

That rule is right, and these tests break it: the prompt has no `--json`, and
what it *says* is the thing being checked.

The difference is that here the prose is the contract. §1.1 makes the
confirmation's content a requirement — it must name what goes — rather than a
presentation detail. "The prompt mentions the recording" is a claim about
behaviour that happens to be delivered as text.

So they go in a file of their own whose header says that, rather than sitting
under a header that forbids them. Two files with opposite rules and a stated
reason is honest; one file quietly contradicting its own preamble is not.

## What these tests must not become

They are being written after the code, against wording that already exists. That
is the position in which a test quietly becomes the specification — somebody
later "fixes" the prompt to match a string a test happened to assert.

So they assert the *claims the spec makes*, not the exact sentences:

- that a recording is mentioned when there is one, and not when there is not;
- that each linked note is named, rather than that the listing has a particular
  shape;
- that nothing was destroyed, checked by reading the note back rather than by
  matching the words "Nothing destroyed".

Where a sentence is quoted it is because the spec quotes it.

## Checking that nothing was destroyed

The useful assertion is about the store, not the output. After declining, the
note still has its text and no redaction is queued — which is what "destroys
nothing" means, and is true whatever the prompt printed.

That matters because the failure worth catching is a prompt that prints
"Nothing destroyed" and queues the redaction anyway. A test reading only stdout
would pass.

## The exact-name cases

Three answers that must not pass: a near miss, an empty line, and the name with
something after it.

The third is the interesting one. `typed.trim() != note.slug` means trailing
whitespace is forgiven — which is right, because a terminal supplies a newline
and some shells add more — but "name yes" is a different answer from "name" and
must be refused. Asserting it pins the difference between tolerating how a
terminal delivers a line and tolerating what somebody typed.
