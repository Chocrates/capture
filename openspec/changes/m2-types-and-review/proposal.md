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

- **`brain-time`** — a new crate. Instants (UTC ms + IANA zone) versus floating
  dates (`YYYY-MM-DD`, never converted), the RFC 5545 recurrence subset, and the
  DST policy §3.2 already fixes. It does not exist in any form.
- **The recognizer** — `brain-parse` holds the vocabulary, the grammar and the
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
  `occ_undone`. `brain-core` currently stops at `UNFLAG(15)`.

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

- **New crates**: `brain-time`. `brain-parse` grows a parser over its existing
  tables and gains its first dependency on `brain-time`.
- **`brain-core`**: new event kinds and bodies, new fold rules, new conflict
  rules. The frozen encoding means every new body is permanent from its first
  write — golden vectors required before anything writes one.
- **`brain-store`**: occurrence expansion, the widened due queue, new projection
  tables and their rebuild path.
- **`brain-desktop`**: calendar and agenda views, the note page's structured half.
- **`brain-cli`**: the same, in text, plus the correction path.
- **`brain-capture`**: untouched by requirement, not by accident.
- **Purity**: `brain-time` must not reach a clock. Recurrence expansion takes
  "now" as a value at the boundary like everything else, or the same log folds
  differently on two machines and §8.3's property test cannot catch it.
