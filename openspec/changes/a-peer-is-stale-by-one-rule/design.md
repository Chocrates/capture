# Design

## Where it goes

`capture_store::syncstate`. That module is already the shared vocabulary for
reporting sync — `sync_state` answers what state this device's syncing is in,
and `lasting` renders a span the same way on every surface. "Has this peer gone
quiet" is the same kind of question about a different subject, and both surfaces
already reach into this module for the second half of the sentence they print.

Not `notifier`, which has its own `STALE_MS` that is also a week and is a
different decision — how late a *moment* may be and still be worth announcing,
with several paragraphs of reasoning behind it. Putting these next to each other
because the numbers match is the mistake this change exists to prevent, pointed
the other way. They are named differently and they stay apart.

## The predicate, not just the constant

Hoisting only the number would leave two copies of

```rust
last_seen.is_none_or(|t| now.saturating_sub(t) > PEER_STALE_MS)
```

which is the decision. The `is_none_or` is the interesting half: it says a peer
never reached counts as quiet, and that is a judgement — the alternative, where
"never reached" is a separate state that is not quite an alarm, is defensible
and is what somebody writing the second copy might reach for.

So the function takes `Option<u64>` and answers the question, and the two
surfaces stop containing the reasoning at all.

## What the surfaces keep

Their wording. One writes a line in a terminal, the other fills a `Condition`
with a tier, and they render the duration with different helpers — `ms` and
`approx`. That difference is not duplication; it is two surfaces saying the same
fact in their own register, which is what §8.4 asks for.

Only the predicate is shared. The sentence is not.

## What this says about `duplicated.py`

The gate claims this category — "a decision made twice is a decision that can be
made differently" — and misses this one by construction, because it compares
function *bodies* and this was a constant and an inline expression.

Worth recording rather than fixing here. Catching constants would need the gate
to compare values across crates and would fire on every `7 * 24 * 60 * 60 *
1000` in the tree, including the two that are genuinely unrelated. The honest
version is a gate that cannot see everything, and a note saying which things.
