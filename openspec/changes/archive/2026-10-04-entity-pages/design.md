## Context

The entity page works already. `mentioning(sigil, value)` (`projection.rs:1734`)
is the filtered timeline, `mentions(tgt, sigil, value)` (`projection.rs:179`) is
the extraction behind it, the desktop draws both the page and a wiki sidebar
(`ipc.rs:573`, `ipc.rs:689`), and `capture mentions @sarah` answers from the
command line. The comment above `mentioning` says it plainly: "There is no entity
record to create and nothing to maintain."

That is true and it is the problem. With the name as the key, §4.3's four named
failures are all reachable: renaming forks the page, two people called Sarah are
one row, there is no merge path, and `@Sarah` is a third person again because the
tokenizer does not fold case (`token.rs:108`).

Three constraints in the code shape everything below.

- **The projection schema may add tables, never columns** (`projection.rs:103`).
  The rule exists because the schema is applied before the version is read, so
  `IF NOT EXISTS` silently skips an added column and the index over it then fails
  to open — making the rebuild that would fix it unreachable.
- **Encoding is frozen from first write** (`event-log/spec.md:9`). A new kind
  needs a golden vector before anything writes one, and `is_known()` is a
  hardcoded `(1..=29)` with two permanence tests over it.
- **The FFI does not take generic queries.** `lists/design.md:118` refused to
  expose `mentioning()` because "composing them there is Kotlin deciding
  something, which `make android-decides` exists to prevent."

## Goals / Non-Goals

**Goals.** Stable identity under renaming. Several spellings reaching one entity.
Merge that loses nothing. Backing notes. Entity pages on the phone. Case folded
for identity, preserved for display. `MuteScope::Entity`. Requirements for the
behaviour that already ships unspecified.

**Non-goals.** Model-based extraction (§2.5.1's `entities` job class). A `client`
kind. Automatic merging. Un-merging. Machine links. Storing `priority`/`project`.

## Decisions

### `ent_id = H("entity" ‖ kind ‖ casefold(first spelling seen))`

The design says `ent_id` is "stable; survives renaming" *and* that pages "exist
for every person and project immediately". Those pull opposite ways: an id minted
by an event cannot exist before the event, and an id derived from the current name
forks on rename — which is the failure the paragraph is arguing against.

Derived from the **first spelling seen** satisfies both. No event is needed for an
entity to exist, and renaming changes `display` while the id stays put, because
the id was never a function of the current name.

"First" must be a property of the event set, not of arrival order, or two devices
would derive different ids from one log. It is the capture with the lowest
`(hlc, id)` among those naming the entity — the same total order the fold already
uses everywhere, and the same tie-break.

*Rejected:* mint a random `ent_id` in an `entity_create` event on first sighting.
It makes every mention a write, turns capture into something that appends two
events, and gives a partitioned pair of devices two entities for one name with no
way to tell they should be one. The whole point of this axis is that it costs
nothing at capture time.

*Rejected:* hash the current name. Renames fork, which §4.3 spends a paragraph
rejecting.

### Names resolve through a table; `mentions` keeps its shape

§1.4 draws `mentions(tgt, ent_id)`. The code has `mentions(tgt, sigil, value)`
with an index the comment calls "the one that has to be fast".

Rather than re-key it, add `entity_names(kind, name, ent_id)` and leave `mentions`
as the raw extraction it already is — which is what §1.4's own comment calls it
("`mentions` is the extraction"). An entity page becomes: resolve the name to an
`ent_id`, follow `merged_into` to its end, gather every name for that id, and
union the existing fast query over those names.

Three reasons this beats re-keying:

- **Adding a table is allowed and rewriting one is the forbidden shape.** Keeping
  `mentions` as it is keeps the migration to a `PROJECTION_SCHEMA` bump.
- **Extraction and identity stay separable.** A mention is what the text says; an
  entity is who that turned out to be. Re-keying would make the extraction depend
  on the merge graph, so a merge would have to rewrite mention rows — a write path
  that can be wrong, over data that is supposed to be derived.
- **A merge stays a redirection.** The spec requires that merging move no data.
  With the indirection outside `mentions`, that is true by construction rather
  than by discipline.

### Three kinds, 30–32, all keyed by `ent_id`

`entity_rename(ent_id, display)`, `entity_merge(from, into)`,
`entity_bind(ent_id, note_tgt: Option)`. `Option` covers unbinding for the reason
`ParentBody` does: "no longer under anything" has to be able to beat an earlier
value, and a kind whose body carries `None` can mean nothing else.

Keyed by `ent_id` rather than a spelling, so a later rename cannot orphan an event
that was about the entity rather than about the word.

`entity_bind` also needs `note_tgt` to be a note that exists; a bind naming an
unknown note is held rather than applied, matching `apply_alias`'s `Held::NoTarget`.

### Entity events are global to the incremental fold

`incremental.rs:104` `referenced()` maps an event to the notes it touches, and
`:141` `is_global()` marks the ones that touch everything. An entity event names
no note at all, so without handling it would re-fold nothing and its effect would
not appear until a full rebuild.

They are global. A merge changes which page every note naming either spelling
appears on, which is not expressible as a set of `tgt`s without walking the
mentions table — and `is_global` already exists for exactly the targetless case
(a `MUTE` with no target).

*Rejected:* resolve the affected notes at fold time and treat the event as
referencing them. It makes the incremental path depend on the projection it is
about to write, which is the circularity the rebuild rule exists to avoid.

### A merge cycle is broken like a parent cycle

Two devices can merge A→B and B→A while partitioned. `fold.rs:1130`
`break_parent_cycles` already solves this shape: detect after the main pass and
drop the newest edge by `(at, id)`.

The same function shape, the same tie-break, for the same reason — a cycle must
not make an entity unaddressable, and the resolution has to be identical on both
devices.

### Case folds for identity, not for display

`casefold` applies to the key and to `entity_names.name`. `display` holds the
spelling first written, so `@Sarah` shows as `Sarah`.

This changes existing behaviour: `@Sarah` and `@sarah` are two rows today and
become one entity. That is a projection-level change, which the schema bump
already forces a rebuild for, and no event changes.

### The FFI gets operations, not a query

`people()`, `projects()`, `entity(name)`, `renameEntity`, `mergeEntities`,
`bindBacking`. Not `mentioning(sigil, value)`.

`lists` already made this call and the reason holds: a phone that composes
`mentioning('@', name)` is a phone deciding that `@` means a person, which is a
decision the store should own. `make android-decides` exists to catch exactly
that.

## Risks / Trade-offs

- **The schema bump rebuilds every store.** `PROJECTION_SCHEMA` 7 → 8, and a
  rebuild folds the whole log. → The designed path (§1.3 makes rebuild the
  migration strategy), already exercised by seven previous bumps, and `state.db`
  is disposable by construction. Cost is one slow first open.
- **Merging is not undoable from a surface.** → `merged_into` is a pointer and the
  inverse is expressible as an event, so nothing is lost; only the gesture is
  missing. Named in non-goals rather than left as a surprise.
- **First-sighting identity is order-sensitive in one visible way.** The display
  name comes from whichever capture is earliest by `(hlc, id)` — which on a
  partitioned pair may not be the one written first by the clock. → Only the
  *display* is affected, never the id, and a rename fixes it. Stating it beats a
  reader discovering it.
- **Case folding will join entities somebody meant to keep apart.** Two projects
  written `^Migration` and `^migration` become one. → Almost always right, and
  where it is not, the two were indistinguishable in prose anyway. No un-merge
  surface makes this the sharpest edge in the change.
- **An entity page over a large archive is a union over several names.** → The
  existing index is on `(sigil, value)` and the name set is small; measured before
  it is assumed, as `read_as` had to be.
- **Three new kinds are three frozen encodings.** A mistake in a body is
  permanent. → Golden vectors first, before any writer exists, which the
  capability already requires.

## Migration Plan

- **Log:** nothing. No event is rewritten. Kinds 30–32 are additive, and an older
  build holds them with a recorded reason and re-evaluates after upgrade, which
  `event-log` already requires.
- **Projection:** `PROJECTION_SCHEMA` 7 → 8 with its changelog line. Every store
  rebuilds on next open; `Session::open` (`session.rs:171`) and the FFI cold-open
  path (`lib.rs:576`) both already do this.
- **Rollback:** revert the code. A store rebuilt by the new build re-opens stale
  against the old one and rebuilds back, because the version is a plain equality
  check.

## Open Questions

- **Does `#` become an entity, or stay a tag?** It produces a mention today and
  `tagged()` unions mentions with the OR-Set. Treated here as a tag with a page,
  not an entity with identity — renaming or merging tags is a different argument,
  and the `client` kind is out of scope. Worth revisiting if merging tags is
  wanted.
- **Where do people and projects live on the phone?** The tab row is full; the
  Pages tab is the closest relative and currently lists only aliased notes.
  Assumed: the Pages tab gains people and projects as sections, matching what the
  desktop sidebar already does.
- **Should a backing note be excluded from its own page's timeline?** It would
  otherwise appear twice, once above and once in the list. Assumed excluded from
  the list, since it is already on screen.
