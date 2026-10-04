## Why

The pre-redaction snapshot is written, retained for thirty days, advertised on
the status page — and nothing can read it.

DESIGN §1.2 says what it is for:

> **Pre-redact snapshot: on, 30-day expiry.** Issuing a `redact` first writes a
> local snapshot of the affected notes, which expires automatically. This plugs
> the hole redaction opens — **a misfire is recoverable during the window in
> which you would notice it** — while leaving redaction itself genuinely
> permanent.

And §1.2 again, on why it exists beside the cancellation window: *"this exists
for the case where you notice a month later."*

Every part of that is built except the part that recovers anything.
`take_snapshot` is called from `purge.rs:131`. `expire_snapshots` runs on every
session open. `snapshot_count` is reported by `capture status`: *"N copy(ies) of
destroyed content are kept locally, and expire on their own."* `Ops::snapshot`
— the read — has **no caller outside tests**.

So the arrangement today is the worst of both: a plaintext copy of content
somebody declared must not exist is kept for thirty days, the person is told it
exists, and there is no way to use it. The cost is paid in full and the benefit
is unreachable.

It matters more than it looks, because the other half of the safety net is
smaller than the design believes. §1.1 says destruction happens six hours after
the redaction's HLC, in five separate places; `REDACT_WINDOW_MS` is **thirty
minutes**. The code is honest about it — `capture redact` prints "It cannot be
undone after 30 minutes" — but it means the cancellable window is a twelfth of
what the design of record promises, and the snapshot is what §1.2 says covers
everything after it.

Found by auditing DESIGN.md's present-tense claims against the code, after two
earlier ones turned out false the same way.

## What Changes

- **`capture recover`** lists what can still be recovered: which redaction, when
  it was taken, how long it has left, and the first line of what it holds.
- **`capture recover <id>`** writes the text back as a **new capture**, and says
  so. The destroyed event stays destroyed — nothing resurrects an event, and
  pretending otherwise would be a worse lie than the current silence.
- **It says what it is.** The listing names it as a copy of content somebody
  ordered destroyed, because that is what a person needs to weigh when deciding
  whether to bring it back.
- **The DESIGN drift is corrected**, not papered over: §1.1's six hours becomes
  thirty minutes in the five places it is stated, because the code is right and
  the document is not.

## Capabilities

### Modified Capabilities

- `cli`: that a misfired redaction can be recovered while its snapshot lives.

## Impact

- `crates/capture-store/src/ops.rs` — listing the snapshots that are still
  alive, which nothing can currently ask.
- `apps/capture-cli/src/{args,ops,write,main}.rs` — the command.
- `DESIGN.md` — the window, in five places.

## Non-goals

- **Resurrecting the event.** The body is gone from the log and the redaction is
  retained so that it destroys matching events that arrive later. A recovered
  note is a new capture with the old text, and the command says so rather than
  implying the redaction was undone.
- **Recovering on the phone or in the window.** The command line is where
  redaction's other recovery routes already are (`redact --cancel`, `restore`).
  §8.4 wants this in the window eventually and that is its own change.
- **Changing the retention or the window.** Thirty days and thirty minutes are
  what the code does; this change makes one of them usable and makes the
  document tell the truth about the other.
