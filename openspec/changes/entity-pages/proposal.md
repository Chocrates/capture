## Why

An entity page already works, on two of three surfaces, and rests on nothing.

`mentioning(sigil, value)` is the filtered timeline §4.3 describes, the desktop
draws it and a wiki sidebar of people, projects and tags, and `capture mentions
@sarah` answers from the command line. None of it is specified: `openspec/specs/`
says nothing about mentions or entities, so the one organising axis that
populates without you is also the one axis with no requirements.

Underneath, a mention is a raw string. `mentions` is keyed `(tgt, sigil, value)`,
which makes four things impossible, and §4.3 names all four as the reason
entities have identity at all:

- **Renaming forks the page.** `@sarah` becoming `@sarah-chen` produces a second
  entity, and the first one's history stays behind under the old spelling.
- **Two people called Sarah are the same person.** There is nothing to tell them
  apart, because the name *is* the key.
- **There is no merge path.** Extraction that produces both `@sarah` and
  `@sarah-chen` leaves two pages and no way to say they are one.
- **Case forks it too.** `@Sarah` and `@sarah` are unrelated rows today; the
  tokenizer does not fold case on a sigil value.

And the phone has none of it. `capture-ffi` exposes neither `mentioning` nor
`distinct_mentions`, and Android's only mention-aware surface is the sigil
cheat-sheet under the capture field. The device where capture happens cannot
answer "what do I know about this person", which is the question the axis exists
to answer.

Two open requirements are waiting on the same thing. `m1`'s redaction spec says
mute SHALL cover an entity; `MuteScope` has `Note | Tag | DateRange` and
`m1/tasks.md:103` defers the variant because "M1 has no entity extraction, so
there is nothing to scope to". There is now.

## What Changes

- **Entities gain identity.** A new `entities` table carries `ent_id`, `kind`,
  `display`, `note_tgt` and `merged_into`, exactly as §1.4 specifies.
- **`ent_id` is derived from the first spelling seen, and never recomputed.** So
  an entity exists the moment a note names it — no event, no curation, pages on
  day one — while the id survives every later rename.
- **Names resolve through a table, not by equality.** A new `entity_names` maps
  `(kind, normalised name) → ent_id`, so several spellings can reach one entity.
  `mentions` stays the raw extraction it already is; nothing about the fast path
  changes shape.
- **Case is folded for identity and preserved for display.** `@Sarah` and
  `@sarah` become one entity whose `display` is the spelling first written.
- **Three new event kinds**, 30–32: rename an entity, merge one into another,
  bind a backing note. Each is a person's decision and none is inferred.
- **A backing note** — the note that *is* the project rather than one mentioning
  it — shows above the filtered timeline.
- **`MuteScope` gains `Entity`**, closing the `m1` requirement.
- **The phone gets entity pages**: people and projects to browse, a page per
  entity, mentions tappable in note text, and rename, merge and bind reachable.
- **The existing behaviour gets specified.** The `mentions` table, the entity
  page as a filtered timeline, and the sigils that produce one are written down
  as requirements rather than left as code.

**BREAKING (projection only, not the log):** `PROJECTION_SCHEMA` goes 7 → 8.
Every existing store rebuilds on next open, which is the designed path — §1.3
makes rebuild both the recovery and the entire migration strategy, and `state.db`
is disposable by construction. No event is rewritten and no capture is touched.

## Capabilities

### New Capabilities

- `entities`: what an entity is, how `ent_id` is derived and stays stable, how
  several names reach one entity, what rename, merge and backing notes mean, and
  what an entity page contains.

### Modified Capabilities

- `browse`: the entity page becomes a specified surface rather than an
  unspecified one, and is required on the phone as well as the window — the same
  reachability argument §8.4 already makes for every mutation.
- `projection`: the `mentions` extraction and the new `entities` /
  `entity_names` tables are specified, including that a redacted note names
  nobody.
- `event-log`: three new kinds, with the golden vectors the capability already
  requires before anything writes one.
- `recognizer`: which sigils produce an entity is stated. `@`, `^` and `#` do;
  `>`, `!` and `~` do not, because they are time, priority and duration rather
  than something with a page.

## Impact

- `crates/capture-core/src/event.rs` — kinds 30–32, `is_known()` widened to
  `1..=32`, and both permanence tests updated.
- `crates/capture-core/src/body.rs` — three bodies, plus `MuteScope::Entity`.
- `crates/capture-core/src/fold.rs` — `Projection` gains entity state, three
  `apply_*` arms, and name resolution that runs inside the main pass for the
  reason `apply_alias` does.
- `crates/capture-core/tests/golden.rs` — vectors for the new kinds, written
  before anything writes one.
- `crates/capture-store/src/incremental.rs` — `referenced()` and `is_global()`.
  An entity event names no note, so without this an incremental fold would not
  know what to re-fold.
- `crates/capture-store/src/projection.rs` — two tables, the `write_into` fill,
  the `write()` and `merge()` delete lists, `PROJECTION_SCHEMA` 7 → 8 with its
  changelog line, and entity-aware reads.
- `crates/capture-store/src/mutate.rs` — writers for rename, merge and bind.
- `crates/capture-ffi/src/lib.rs` — operation-shaped calls, not a generic query.
- `android/app/src/main/java/dev/capture/` — an entity page, a people/projects
  index, tappable mentions, and the three mutations.
- `apps/capture-desktop`, `apps/capture-cli` — the same answers through the
  existing seams, now entity-aware.

## Non-goals

- **Entity extraction by model.** §2.5.1 makes `entities` an enrichment class
  wanting "a small model or gazetteer" — noticing "coffee with Sarah" with no
  sigil. That is how names get *found*; this change is what a page *is*. Nothing
  here depends on a model, and bundling them would put one inside a change that
  needs none.
- **A `client` entity kind.** §1.4 lists `person | project | client`, and `#` is
  a tag in the code today. Tags already have a surface and an OR-Set; giving
  them a second identity is its own argument.
- **Automatic merging.** Nothing guesses that two names are one person. §3.6's
  lesson about `carries_forward` applies: two spellings are indistinguishable
  from two people by phrasing alone, so merging stays a gesture.
- **Splitting a merged entity.** `merged_into` is followed, not undone. A merge
  is reversible in principle by writing the inverse, and a surface for it can
  wait until one is wanted.
- **Machine links and the "related" layer.** §4.3's third link kind needs
  embeddings and stays out.
- **`Markers.priority` and `Markers.project` as stored fields.** Still parsed and
  still dropped. `^project` reaches an entity page through the extraction; giving
  priority a home is a separate change.
