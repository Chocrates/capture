## Context

`read.rs`'s JSON section opens with the reason the JSON is hand-written rather
than derived: "a serialiser attached to the storage types would make every
future field rename a silent breaking change to someone's pipeline." That
reason stands, and nothing here derives from storage types.

What it did not anticipate is the cost of the other half of the decision. The
prose and JSON paths are two independent pieces of code reading the store, and
nothing connects them. The prose path is the one a person looks at while
writing the command, so it is the one that gets the new field. Four fixes so far
have each patched one command's JSON and left the next to drift.

## Goals / Non-Goals

**Goals:**
- A fact the prose can show is a fact the JSON carries, enforced by the compiler.
- Every `--json` output is valid JSON, enforced by a test that parses it.
- Close the drift found in the audit.

**Non-Goals:**
- Serde on storage types, for the reason above.
- Making prose show everything the JSON carries. JSON may carry more —
  millisecond timestamps, ids at full length — because a script can use them
  and a person cannot. The guarantee runs one way.
- Writing commands' JSON (`capture + … --json` does not exist). Out of scope.

## Decisions

### 1. A view per command, and a prose renderer with no store

Each command splits into `build(store, …) -> View` and two renderers. The prose
renderer's signature is `fn prose(v: &View)`, with no `&Store`, so the only
facts it can print are ones the builder put in the view.

This is the guarantee that matters, and it holds without anybody remembering
it: to print something new, the prose renderer has to get it from the view;
to get it into the view, a field is added; and adding a field breaks the JSON
renderer, which is next.

### 2. The JSON renderer destructures exhaustively

```rust
fn json(&self) -> Json {
    let TaskView { slug, descriptor, band, due, text } = self;
    obj([("slug", slug.into()), ...])
}
```

No `..`. A field added to `TaskView` is a compile error here until it is
named. That makes leaving one out a decision visible in the diff rather than a
default.

*Alternative considered:* a test comparing prose and JSON output. Rejected —
prose is formatted for people (truncated, dated, worded), so the test would
have to know how every field renders, and it would only catch drift in the
cases somebody thought to set up.

### 3. A JSON value type, not `format!`

`enum Json { Null, Bool, Int(i64), Str(String), Arr(Vec<Json>), Obj(Vec<(&'static str, Json)>) }`
with one `render`. Keys are `&'static str`: a key is part of the interface
and should be visible in source, never computed. Objects keep insertion order.

`quote` moves here unchanged; `ops.rs` and `write.rs` call it today and keep
doing so through the new path.

### 4. Two shapes become objects

`due` and `peers` have a fact about the whole answer — whether captures are
unread, whether any sync has ever run — that has nowhere to go in a bare array.
`tasks` and `agenda` already made this change for the same reason, and the
spec already requires the incompleteness report in JSON. The key holding the
rows is named for them (`"due"`, `"peers"`), matching `"tasks"` and
`"entries"`.

### 5. `muted` means in force

`"muted": true` for a mute that ended is the bug `is_muted(now)` exists to
prevent, and the prose already uses it. `muted_until` is added (`null`, `0` for
indefinite, otherwise milliseconds) because the prose of `show` prints the end.

## Risks / Trade-offs

- **The refactor touches every read command at once** — the reason it was
  deferred four times. Mitigation: the prose output is meant to be byte-for-byte
  unchanged except where this change says otherwise, and the existing suites
  assert on prose heavily. A prose diff is a regression unless listed here.
- **Two output shapes change.** Mitigation: nothing in the repository reads
  them; the change is stated in the proposal.
- **A view for a small command is ceremony.** Accepted for the commands where
  the two renderings already agree (`log`, `verify`, `rejected`, …): the point
  is that the next field added to them cannot drift either.
