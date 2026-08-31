## Why

M1 answers "am I capturing." It cannot answer "does this help" — every capture is
text, and nothing reads it. `DESIGN.md` §8.10 is explicit that this is the
milestone where the design first becomes testable against its own premise rather
than merely usable.

It is also where the evidence-backed components land. An earlier build order put
scheduled review, intents and activation in M5 — after sync, after Android — and
a second adversarial review pointed out that this made the project's own stated
worst case structurally certain: the design would have *named* the risk that
protocol work displaces the components with evidence, and then *built* that risk.
None of the three needs sync, Android or enrichment (§8.10). They are here
because they are cheap here.

## What Changes

- **`capture-time`** — a new crate. Instants (UTC ms + IANA zone) versus floating
  dates (`YYYY-MM-DD`, never converted), the RFC 5545 recurrence subset, and the
  DST policy §3.2 already fixes. It does not exist in any form.
- **The recognizer** — `capture-parse` holds the vocabulary, the grammar and the
  confidence bands (§3.1.5) as tables and nothing that reads them. This adds the
  parser: tier 1 sigils, tier 2 grammar, the guards that keep it off ordinary
  prose, and a confidence score against the 0.75 floor.
- **Promotion** — a `promote` event assigns a derived type (event, task,
  recurring series) with its fields. Additive and reversible: the captured text
  is never rewritten, consumed or relocated (§3.1.3).
- **Recurrence** — the rule is stored, occurrences never are. Lazy expansion into
  the projection over a rolling horizon, with per-occurrence exceptions (§3.4,
  §3.5).
- **Scheduled review** — resurfacing on a schedule, recording *shown / engaged /
  deferred* and never a graded pass or fail (§2.7, §2.8).
- **Intents** — if-then plans with a cue spec and an action, firing locally on
  this device (§2.4). Cues available without a phone: time, `app_focus`,
  `git_branch` (§2.4.1).
- **Activation** — meetings and contexts surfacing the notes captured against
  them, using the recurrence machinery plus one event kind (§3.9).
- **Calendar and agenda** — the views M1 deliberately excluded (`DESIGN.md`:3796).
- **New event kinds** — `promote`, `intent`, `review`, and the occurrence
  exception family `occ_move` / `occ_edit` / `occ_cancel` / `occ_done` /
  `occ_undone`. `capture-core` currently stops at `UNFLAG(15)`.

## Capabilities

### New Capabilities

- `time-model`: instants versus floating dates, the IANA zone stored beside every
  instant, and the DST policy for expansion — nonexistent local times shift
  forward, ambiguous ones take the first, both recorded on the occurrence rather
  than applied silently (§3.2).
- `recurrence`: the stored rule as truth, lazy occurrence expansion over a
  rolling horizon as derived projection data, occurrence identity, and the
  exception events that modify a single occurrence without touching the series
  (§3.4, §3.5).
- `recognizer`: the three tiers in order, the closed vocabulary and grammar of
  §3.1.5, the guards that keep a match off ordinary prose, the 0.75 confidence
  floor, and the suggestion tier below it (§3.1, §3.1.1, §3.1.2, §3.1.5).
- `promotion`: assigning a derived type without rewriting the capture, the
  three-part note (`text` / `fields` / `descriptor`), reparse on edit, and human
  corrections that no later machine parse may override (§3.1.3, §3.1.4, §6).
- `review`: scheduled resurfacing, the outcomes it records, and the explicit
  refusal to construct a pass/fail signal (§2.7, §2.8, §3.6).
- `intents`: if-then plans, the cue specifications this device can evaluate, and
  local firing with the notifier selection §2.4.1 already settles.
- `activation`: a meeting or context surfacing what was captured against it,
  built on the recurrence machinery and one event kind (§3.9).

### Modified Capabilities

- `event-log`: adds `promote`, `intent`, `review` and the five `occ_*` kinds,
  each with its §6 conflict rule — including the one that is not plain LWW,
  where a human `promote` beats a parser-authored one regardless of HLC.
- `projection`: occurrence expansion into `state.db` over a rolling horizon,
  rebuildable and never authoritative; the due queue growing to carry review and
  intent deadlines alongside redaction.
- `browse`: the note page gains type, fields and the regenerated descriptor
  shown beside the verbatim text; the calendar and agenda surfaces arrive.
- `cli`: surfaces for the derived types and the agenda, and the correction path
  that pins a human promotion.

## Non-goals

- **The capture gesture does not change.** §8.10 requires the chord, the overlay
  and the commit key to be frozen for the duration, because context stability is
  among the strongest accelerators of habit formation and this build changes
  surfaces repeatedly across exactly the window where stability matters most.
  Everything here happens *after* the commit.
- **Tier 3, the model fallback** (§3.1.2). It needs a capable device awake and a
  model present, which is M5 enrichment. Tier 2 must decline gracefully to a
  plain note rather than wait for it.
- **Sync** (M3). Still one device. The conflict rules for the new kinds are
  specified and property-tested here, and exercised across devices later.
- **Android, location cues, phone-as-notifier** (M4). Intents fire on the desktop,
  which is where `app_focus` and `git_branch` cues belong anyway (§2.4.1).
- **Enrichment** — transcription, entities, links, the specificity pass (M5).
- **Entities on the note page.** M1 left that section unbuilt deliberately; it
  stays unbuilt until something can populate it.

## Impact

- **New crates**: `capture-time`. `capture-parse` grows a parser over its existing
  tables and gains its first dependency on `capture-time`.
- **`capture-core`**: new event kinds and bodies, new fold rules, new conflict
  rules. The frozen encoding means every new body is permanent from its first
  write — golden vectors required before anything writes one.
- **`capture-store`**: occurrence expansion, the widened due queue, new projection
  tables and their rebuild path.
- **`capture-desktop`**: calendar and agenda views, the note page's structured half.
- **`capture-cli`**: the same, in text, plus the correction path.
- **`capture-overlay`**: untouched by requirement, not by accident.
- **Purity**: `capture-time` must not reach a clock. Recurrence expansion takes
  "now" as a value at the boundary like everything else, or the same log folds
  differently on two machines and §8.3's property test cannot catch it.

## Outcome

Written at close, against what was proposed rather than from memory.

### What shipped beyond the proposal

Four things were built during M2 that this document never asked for, all in the
browse surface and all in response to using it:

- **The wiki layer.** `[[links]]` were extracted, resolved and indexed both
  ways, and the note text showed the raw brackets — the layer existed and was
  unreachable. Links are followable now, with three states rather than two (it
  resolves, the name moved, or nothing has that name yet), navigation keeps
  back and forward across every page, and editing happens in place with `[[`
  completion instead of in a browser dialog.
- **Entity pages.** `@person`, `^project` and `#tag` reach a page of everything
  naming them. `#` was the one that mattered: it was collected by the
  recognizer and dropped, so the single organising axis somebody writes by hand
  was the only one that never appeared anywhere.
- **A wiki pane**, with a sidebar built entirely from the store — pages are the
  notes somebody aliased, and the rest is whatever the notes say.
- **Nested pages**, which is the one place this design files by hand. §3.7's
  argument against it is not withdrawn; it was asked for with the argument in
  view. The containment is that nothing outside the wiki sidebar reads a
  parent, so a page in the wrong branch is misplaced in one list and is still
  in the timeline, in search, on every entity page naming it and behind every
  link to it. That is a test, not a claim.

### What was deferred, and why

- **8.2 — the active-project selection tier.** Needs a notion of an *active*
  project, which does not exist: `^project` is a sigil the recognizer collects
  and nothing makes current. Building a guess would put a tier in the review
  order that fires on the wrong things.
- **9.2 — the cue types this device cannot observe.** `app_focus` is not
  observable on GNOME/Wayland at all, and `location` needs a phone. Both are
  refused at creation rather than accepted and silently never fired, which is
  the honest half of the requirement.
- **10.2 / 10.3 — external calendar import.** No feed, no importer, nothing to
  test an ICS or CalDAV reader against. §3.9's value is the *refusal* to write
  back, and nothing yet needs it. A parser written against an imaginary feed
  would have been the most speculative code in the milestone.
- **11.1 — a test per spec scenario.** Every capability has runnable tests and
  the load-bearing scenarios are covered. What is not covered is the scenarios
  of the things above, which do not exist — claiming otherwise would be
  claiming coverage of absent code.
- **Entity identity** (§4.3's `ent_id`, `display`, `merged_into`) is not built.
  Renaming a person forks their page, two people with one name share a page,
  and there is no merge path. Mentions are derived data, so adding identity
  later is a projection change rather than a log one — which is what made
  deferring it safe.

### What the milestone taught, which was not what it set out to teach

Six defects were found in the last stretch and every one came from running the
software against a real store rather than from reading it:

1. `is_stale()` had no caller, so nothing rebuilt a projection after a schema
   bump — while this document assumed rebuild-not-migrate throughout.
2. A reparse re-resolved relative expressions against the sweep's clock, so
   every grammar bump walked "meeting at 14:00" forward a day.
3. Review's answers lived in the page, so leaving and returning offered them
   again — and `deferred` accumulates, which retires an item permanently.
4. A `#tag` typed into a capture reached nothing.
5. A link to a name matching nothing was drawn as followable.
6. Adding a column to `notes` made every existing store unopenable, and the
   rebuild that would have fixed it starts by opening.

Three of those shared one cause: a query spelling out its columns instead of
using the shared list. The rule that came out of it — **the schema may add
tables, never columns** — is now written in the schema itself and pinned by a
test, because rebuild-not-migrate depends on an old projection staying openable
long enough to be found stale.
