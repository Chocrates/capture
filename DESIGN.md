# Second Brain — Design

Single user, **n devices** — starting with a Linux desktop, a Mac laptop, and an Android phone,
but the design assumes devices are added and retired freely. No cloud, no central coordinator,
no server to operate. Every device holds the same complete dataset. Devices exchange events
directly over the local network.

This document covers the **capture subsystem** (the part the *category's observed failure mode*
points at — not, as an earlier draft claimed, the part "the evidence says" is broken; see §0.0),
the **domain model** for time, tasks and organisation, the **views** over it, **BSP** — the sync
protocol, with its failure modes and conflict semantics — and the **implementation plan**.

Implementation is Rust: one cargo workspace, one shared core, thin platform shells (§8).

**The evidence this design claims to derive from is recorded in `EVIDENCE.md`**, with citations,
effect sizes, and the sample-population limitations that §0 currently omits. An adversarial
review found several of §0's derivations overstated; read that file before relying on any claim
in §0.

---

## 0. The premise

### 0.0 What the evidence does and does not license

An earlier version of this section claimed more than the research supports, and since everything
downstream leans on it, the honest boundaries come first. Full citations, effect sizes and sample
populations are in `EVIDENCE.md`.

**No study cited here tested a knowledge-management tool.** Not one.

But an earlier draft continued "it says nothing about whether an app like this helps, in either
direction", and **that is now false and was self-serving.** The literature says a great deal about
software for ADHD adults: 23 RCTs, N=1,472, **g = −0.32** on overall symptoms, and the attexis
trial at d = 0.71–0.98 (`EVIDENCE.md` §2.4). What it says is that software helps **when it
delivers a structured therapeutic programme** — CBT modules, activity scheduling, implementation
planning. Not when it stores notes.

"No evidence either way" is not a humbler version of that; it is a *different* claim, and it is
the one that leaves this design's shape unchallenged. "There is evidence and it points at features
we did not build" demands something. See §9 Q0, where it now does.

So the following claim, which this document used to make, is withdrawn:

> ~~The standard second-brain product assumes the bottleneck is organisation. The research says
> otherwise.~~

The research does not say otherwise. It was being used to discredit a competing premise while
this design's own premise went untested by the same standard, and that asymmetry was the central
dishonesty of the original §0.

What can be said honestly is narrower and still worth acting on: **the category's observed
failure mode is that capture does not happen**, and the cognitive evidence gives specific,
checkable reasons why capture is expensive for this user in particular. That justifies designing
around capture cost. It does not prove the resulting tool works.

Four further limits, stated once here rather than repeated:

- **No paper in this evidence base was read in full.** All of it comes from web-search result
  summaries; every effect size below is "as reported by a summary", not verified against the
  paper. This is the largest limitation on everything that follows, `EVIDENCE.md` §0 states it
  first for that reason, and an earlier draft of this section omitted it while presenting six
  numbered findings with journal-level authority.
- **Some of the best-supported interventions are not software.** Cognitively demanding exercise
  and medication both have support, though the medication claim needs care: an earlier draft said
  medicated adults were "statistically indistinguishable from controls", which is one study on
  specific memory measures. Meta-analytically, stimulant effects on *cognition* in adults are
  **small** — working memory g = 0.13, memory 0.25 — while effects on *symptoms* are much larger
  (`EVIDENCE.md` §5). Strong for symptoms, modest for cognition, and still not software.
- **One large effect *is* software-deliverable and absent here.** Behavioural activation:
  g = 0.85 individual, g = 0.69 internet-delivered, indistinguishable from CBT across 15 RCTs. It
  is not in this design at all. See §9 Q0.
- **Several findings are from children or college students**, not adults. Where that matters it
  is flagged inline below.
- **Effect sizes here are small.** g = 0.31 for implementation intentions is a small effect, and
  OGM's predictive effects are described as small. Nothing in this evidence base warrants
  confident claims.

### 0.1 The findings, with their limits

1. **The adult ADHD memory deficit is at encoding, not retrieval** — Skodzik, Holling & Pedersen
   (2017), meta-analysis. *Limit the original draft dropped:* the deficit was **verbal**; there
   was **no** deficit on visual long-term memory.

2. **Retrieval practice benefits ADHD as much as controls** — Knouse et al. (2016), college
   students. This is a *null moderation*: ADHD status did not change the testing effect. What
   does differ is **self-directed pacing**, where ADHD students lose ground while imposed
   structure keeps the benefit equal. Also from this line of work: **unmedicated** participants
   were worse at encoding and final test; **medicated** participants were not significantly
   different from controls.

3. **Implementation intentions help, modestly** — Breitwieser et al. (2025), g = 0.31, N=12,957.
   *Limits the original draft dropped, both material:* this is a meta-analysis of **children**,
   and effects were **stronger in younger children** — an age gradient running away from an adult
   user. The original called this "the largest ADHD-specific effect in the literature reviewed",
   which is false on the numbers: c-MeST reports d = 0.53–0.93 and a clinical outcome.
   Additionally, what the studies measured is an **internalised, self-formed** cue→action link;
   a machine-detected push notification is external prompting, which is a related but different
   intervention.

4. **Habits take 66–154 days**, not weeks — Lally (2010), median 66, range 18–254; 2024 review,
   means 106–154. **A single missed day costs under half a point of automaticity and recovers
   quickly.** **Context stability** is among the strongest accelerators.

5. **Overgeneral autobiographical memory** is robust in depression and persists in remission;
   **c-MeST**, a structured clinical protocol, improves specificity (d = 0.53–0.93) and reduced
   MDE criteria at one month (35.7% vs 60.6%). *Limit:* c-MeST is dosed, valence-balanced and
   supervised. It is not the same thing as a note app asking a question.

6. **Anxiety impairs processing efficiency more than effectiveness** — Eysenck et al. (2007).
   Worry preempts working memory. *The half the original draft dropped, which matters for how
   this design is evaluated:* **performance is often maintained at higher cost** — same accuracy,
   more effort.

7. **Reward learning is blunted in depression** — reduced reward-prediction-error signal.
   *Honest caveat:* this does not straightforwardly imply push beats pull. Blunted reward
   learning equally predicts that pushed payoffs are less reinforcing, and that unrewarding
   notifications train avoidance of the channel.

### 0.2 Design rules, and how well each is actually supported

| Rule | Basis | Strength |
|---|---|---|
| Capture takes one gesture and zero decisions | ACT: a choice at peak load is a WM allocation; plus the undesirable-difficulty literature on already-stressed WM (`EVIDENCE.md` §2.2) | **Moderate.** Downgraded from "Strong": ACT is a theory of anxiety and processing efficiency and tested no capture interface, so this is two inferential steps from the finding. It remains the best-supported rule here — which says more about the others. |
| The system performs *retrieval scaffolding* after the fact, not "encoding work" | Finding 1, reframed (§0.3), against the offloading objection (§0.3.1) | Contested |
| Capture never blocks on network, disk, or another device | Observed failure mode of the category, not a study | Engineering judgement |
| The system schedules review **by default**; the user may always pause or slow it | Finding 2 — self-directed pacing degrades *learning outcomes*, which is not the same claim as denying someone a stop button. Retracted and corrected in §2.6. | Moderate |
| Recurring misses do not stack; no streak counters | Finding 4 (missed day ≈ free) plus a plausibility argument about failure signals | **Weak-moderate.** Nothing tested streaks or backlogs. Labelled as a judgement call in §3.6. |
| Output is pushed, not pulled | **External memory aids for prospective memory** (`EVIDENCE.md` §2.3) — people using a memory notebook needed reminder alarms *to remember to check it* | **Moderate**, and better than it was. The original basis was finding 7 (blunted reward learning), whose inference is contestable in both directions. The rehabilitation literature tested the actual behaviour. Caveat: those samples are acquired brain injury and MCI, not ADHD. |
| Capture gestures are as similar as each platform allows | Finding 4, context stability | Moderate — and honestly stated in §2.2, which lists seven surfaces, not one |
| Capture opens in ~150 ms | UI responsiveness convention | **Not research-derived.** See §2.1. |

### 0.3 The encoding correction

The original draft said the system "performs encoding work" and used that to dismiss competitors
for investing in retrieval. Both halves were wrong, in the same way.

Encoding in Skodzik is a **cognitive process in your head at the moment of learning**. Software
cannot do it for you. What enrichment and the ambient envelope actually build — titles, entities,
links, window titles, branch names, locations — are **retrieval cues**. That is retrieval
scaffolding, not encoding.

Which produced a contradiction: §0 attacked the category for investing in retrieval, then this
design shipped tags, entity links, seven browse axes and a search view whose own rationale is
"retrieval is the intact faculty — make it excellent."

The honest framing, which is defensible and needs no contradiction:

> Externalising storage **bypasses** the encoding deficit rather than repairing it. Because
> retrieval is the intact faculty, investing in excellent retrieval surfaces is exactly right —
> they are the part of you the system can lean on. What must be avoided is not retrieval work,
> it is requiring *the user* to do organising work at capture time, when their capacity is
> lowest.

That reframe survives contact with the evidence. The original did not.

### 0.3.1 The objection to all of this: cognitive offloading

"Bypasses rather than repairs" is not settled, and the strongest argument against this entire
design says it may be worse than neutral.

**Cognitive offloading is real.** When people expect information to remain externally available
they remember the information less and its location more (Sparrow 2011). A 2024 meta-analytical
review finds the effect real but strongly moderated; a 2025 meta-analysis reports **small-to-
moderate deficits in working and long-term memory under habitual external access**
(`EVIDENCE.md` §2.1).

**The moderator is the uncomfortable part: perceived reliability of the external store.** This
system is designed to be maximally reliable — replicated to every device, offline-capable,
never loses anything. **So the better it works, the stronger the offloading effect should be.**
That is not a flaw that can be engineered around; it is the mechanism.

Three things can honestly be said in response, and none of them is a refutation:

1. **Offloading what you would otherwise lose is not the same as offloading what you would
   otherwise retain.** The offloading studies use material healthy participants *would* have
   remembered. §0.1 finding 1 says the verbal encoding is impaired in the first place. You cannot
   offload a memory you were never going to form.
2. **The trade may still be worth making knowingly.** Losing some retention of things you can
   always look up, in exchange for having them at all, is a defensible bargain — but it is a
   bargain, and the user is entitled to know they are making it.
3. **It argues against features this design already rejects**, and for one it does not have: it is
   an argument against making the system the *only* copy of anything you care about remembering,
   and in favour of the scheduled retrieval practice in §0.2 — which is the one component with a
   direct evidential mapping and, per §9 Q0, the least-specified thing in the document.

Recorded here rather than in an appendix because it is the deepest objection to the premise, and
because the previous version of §0 discussed the encoding deficit at length without ever
mentioning that externalisation has a documented cost.

### 0.4 One direction the evidence points that this design does not follow

Finding 1's verbal/visual split is unexploited, and it is the most interesting unexplored lead
here. If verbal long-term memory is impaired and visual is intact, a design derived from the
evidence would lean toward the **intact** channel: screenshot capture, image notes, spatial
layout, sketch input, visual browsing. This design is overwhelmingly verbal and converts its one
non-verbal input to text as fast as it can. That is pointing at the deficit rather than around
it. Recorded as an open question (§9) rather than quietly omitted.

---

## 1. Data model

Everything is an immutable event in an append-only log. State is a fold over the log.

**Every device holds one store containing every event from every device.** There is no
per-device file, no sharding, no partitioning of storage. Each device is a complete replica of
the same dataset.

### Nobody owns a note. Every device owns only its own pen.

**You can create, edit, tag, and delete anything from any device.** There is no ownership of
notes, no home device, no "this note lives on the phone." Every device has full write access to
the entire dataset. Say this first because the mechanism below is easy to misread as
partitioned ownership, and it is not.

The property that makes conflicts structurally impossible is **append-only authorship**, and it
constrains exactly two things:

1. A device stamps events with its own `dev` id and never appends into another device's `seq`
   counter.
2. An event record, once written, is never rewritten or erased — by anyone, including its
   author.

Neither constraint is visible to you. Editing is not mutation of a record; it is **appending a
new event that supersedes the old one**. Deleting is appending a tombstone. The note you see is
a fold over every event touching it, regardless of which device wrote which part.

Worked example — one note, edited from three devices:

```
phone    capture  id=A            target=A   "budget 40k, revisit Q2"
laptop   edit     id=B  prev=A    target=A   "budget 40k → 55k, Sarah owns"
desktop  edit     id=C  prev=B    target=A   "…, deferred to Oct planning"
desktop  tag      id=D            target=A   #client-acme
phone    untag    id=E  kills=D   target=A
```

Five events, three authors, one note. All legal. `A` currently reads as `C`, untagged. Nothing
was overwritten and nothing was owned.

Because records are only ever appended, merge is set union — commutative, associative,
idempotent, and needing no merge algorithm at all. (Strictly it is a join-semilattice rather than
a pure grow-only set, because redaction moves a body to an absorbing bottom value; §6 spells out
why that still converges.)

One operational note: a device can only edit a note it has materialised locally. Full replicas
always have everything. A `leaf` device (§5.5) may need to range-fetch an old note before
editing it, and will say so rather than silently presenting a partial history as complete.

```
~/capture/
  events.db        # every event from every origin. Append-only, immutable, replicated.
  state.db         # derived projection (§1.4). Disposable, rebuildable, never synced.
  ops.db           # local operational state. Not derivable, not synced, survives re-fold.
  blobs/<blake3>   # audio, images — content addressed
  export/          # incremental event export (§1.2)
  config/
    identity.key   # this device's static keypair + current incarnation
```

`events.db` is SQLite in WAL mode. This replaces the hand-rolled log framing an append-only
file would need: SQLite already provides atomic commit, crash-safe torn-write recovery,
indexing, and range queries, all of which the sync protocol needs anyway.

```sql
CREATE TABLE events (
  id    BLOB PRIMARY KEY,   -- blake3 over the canonical HEADER plus bhash. Never over body.
  dev   TEXT NOT NULL,      -- authoring origin: H(device_pubkey ‖ incarnation). Not ownership.
  seq   INTEGER NOT NULL,   -- per-origin, gap-free, monotonic
  hlc   BLOB NOT NULL,      -- wall_ms | counter | dev
  kind  TEXT NOT NULL,
  tgt   BLOB,               -- note identity: the root capture this event acts on
  prev  BLOB,               -- mutations: the exact version being superseded
  occ   INTEGER,            -- recurrence exceptions: the occurrence's original_start (§3.5)
  chain BLOB,               -- id of this origin's previous event (§5.3) — per-origin hash chain
  bhash BLOB NOT NULL,      -- blake3 of canonical body, whose field 0 is a 16-byte nonce
  body  BLOB,               -- NULL once redacted (§1.1)
  sig   BLOB NOT NULL,      -- Ed25519 over id, by the origin's device key
  UNIQUE(dev, seq)
) STRICT;

-- Unauthenticated events are NOT stored here. They cannot be permitted to occupy a
-- (dev, seq) position or advance `heads` — see §5.3 and the `rejected` side table.

CREATE INDEX events_hlc ON events(hlc);
CREATE INDEX events_tgt ON events(tgt, hlc);   -- fold one note without scanning
CREATE INDEX events_occ ON events(tgt, occ) WHERE occ IS NOT NULL;

-- Materialised version vector. `seq` is the highest CONTIGUOUS sequence held for
-- this origin — never a maximum. See below; the distinction is load-bearing.
CREATE TABLE heads (
  dev      TEXT PRIMARY KEY,
  seq      INTEGER NOT NULL,   -- highest contiguous seq, not max(seq)
  sealed   INTEGER NOT NULL DEFAULT 0,
  final_id BLOB                -- sealed origins only: id of the final event (§5.5)
) STRICT;

-- Events held above the contiguous head, awaiting the gap to fill.
CREATE TABLE pending (dev TEXT NOT NULL, seq INTEGER NOT NULL,
                      PRIMARY KEY (dev, seq)) STRICT;
```

The `(dev, seq)` pair is the logical stream. It exists as two columns, which is all it ever
needed to be — it is what lets the sync protocol describe "what I have" in one integer per
origin (§5.5).

**Duplicate ingest is a no-op only when the ids also match.** A `UNIQUE(dev, seq)` violation
where the incoming `id` *differs* from the stored one is not a duplicate — it means two different
events claim the same position in one origin's stream, which single-writer-per-origin says is
impossible. It is a loud, quarantined error, never a silent no-op. Treating it as benign is what
turns the restore hazard below into silent permanent data loss.

### `heads[dev]` is the highest **contiguous** seq, never the maximum

This is one line of definition that prevents ordinary use from losing data silently.

Events arrive out of order by construction: live push (§5.4) runs concurrently with anti-entropy
`RANGE` replies (§5.5), and gossip relays any origin's events from any peer. If `heads` were
maintained as `max(seq)`:

```
t0  laptop needs desktop 41501..41822; RANGE begins streaming
t1  user captures on desktop → seq 41823 → live push arrives FIRST
t2  laptop stores it, heads[desktop] = 41823
t3  wifi drops with the RANGE only as far as 41600
t4  laptop reconnects and advertises desktop: 41823
    → no peer ever sends 41601..41822 again
    → 222 notes lost locally, forever, while §4.5 prints "✓ all in agreement"
```

No attacker, no bug, no unusual sequence. The version vector is the system's only completeness
oracle, and treating it as a maximum breaks exactly the property it exists to provide — the same
class of failure as the restore hazard below, reached through a different door.

So: out-of-order events are stored and recorded in `pending`; `heads` advances only across a
gap-free prefix, absorbing pending rows as the gap fills.

### Device incarnation — why `dev` is not just a device name

A restored replica is the sharpest failure this design has, and it hides on the path most likely
to be used in an emergency:

```
t0  desktop at seq 41822; every peer holds desktop:41822
t1  disk dies; restore from a 3-day-old export → local heads[desktop] = 41500
t2  desktop captures before reconnecting (capture never blocks on network,
    §7 mode 1) and allocates seq 41501 — a number already used by a different event
t3  desktop syncs
```

Without a fix: desktop asks for `41502..41822` — never 41501, which it believes it has — so the
original 41501 is lost locally forever while `heads` asserts completeness. Meanwhile the *new*
41501 is refused by every peer as a `UNIQUE` collision and can never propagate. Both sides report
converged. They are not, in both directions, silently and permanently — and the version vector,
the only completeness oracle in the system, is precisely what the rollback corrupted.

**Fix: origin identity includes an incarnation.** `dev = H(device_pubkey ‖ incarnation)`, minted
fresh on every restore, reinstall, or database replacement. A restored desktop is a *new origin*
starting at seq 1. Its old origin is intact, immutable, and still held by peers; nothing collides,
nothing is lost, and anti-entropy backfills the gap between the export and the present.

**The incarnation is a random 128-bit value, not a counter.** A counter looks natural and is
wrong, because it must be read from somewhere — and the thing you read it from is exactly the
thing that was just lost:

```
t0  desktop on incarnation 3; nightly exports
t1  disk dies → restore from last night → incarnation 4 → desktop.4
t2  desktop.4 runs a week, 3000 events, syncs everywhere
t3  disk dies again; only a THREE-WEEK-OLD export survives, taken during
    incarnation 3 → restorer reads "3", increments → desktop.4 AGAIN
t4  the second desktop.4 starts at seq 1 and collides at every position
```

The same collision follows from restoring one export onto two machines, or restoring twice
because the first attempt looked like it failed. An incarnation is a **nonce, not a version** —
there is nothing to increment from, nothing for a stale copy to reproduce, and the exporter does
not need to record it.

Restore therefore has one mandatory step: **mint a fresh incarnation before the first local
append.** §5.5's sealing keeps retired incarnations from accumulating in the version vector.

**Roster membership is keyed by public key, not by origin** (§5.3). Otherwise the restored
`desktop.<new>` has no `device_add`, every peer quarantines everything it writes, and restore —
the path most likely to be used in an emergency — is a lockout.

`state.db` is separate rather than a set of tables inside `events.db` so it can be deleted and
re-folded without any risk of touching the durable artifact, and so `events.db` can be copied
or replicated as a self-contained unit. Its schema is §1.4.

### Event envelope

```jsonc
{
  "id":     "b3:8f2a…",       // blake3 over EVERY field below except sig — dedup + integrity
  "dev":    "desktop.3",       // origin = H(pubkey ‖ incarnation); any device may act on any note
  "seq":    41822,             // per-origin, gap-free, monotonic
  "hlc":    [1755710241371, 3, "desktop.3"],  // wall_ms, counter, origin (total order)
  "kind":   "edit",
  "tgt":    "b3:0a91…",        // note identity — the root capture being acted on
  "prev":   "b3:1c40…",        // mutations only: the exact version being superseded
  "chain":  "b3:77de…",        // id of seq 41821 — this origin's hash chain (§5.3)
  "bhash":  "b3:5e02…",        // blake3 of canonical body — commitment that survives redaction
  "body":   { 0: "9f3c…", … },     // field index 0 is a 16-byte random nonce. Always.
  "sig":    "ed25519:…"        // over id, by origin device key
}
```

**`id = blake3(canonical header ‖ bhash)`** — over every field above except `body` and `sig`,
with the body represented by its hash rather than inlined. The indirection is small and buys a
lot: because `bhash` is stored and never destroyed, a redacted event's `id` is still recomputable
and its signature still fully verifiable (§1.1). Committing to the body directly would mean
redaction permanently broke integrity checking for that row.

**Every canonical body carries a 16-byte random nonce at field index 0, and this is a security
requirement rather than a formality.** Without it, `bhash` is an *unsalted commitment to content that
redaction was supposed to destroy* — and it survives forever, on every replica, in every export
file, in every offsite backup. The two cases §1.1 exists for are a pasted password and a third
party's personal details: both short, both low-entropy, both trivially confirmable by hashing
candidates until one matches. Anyone holding an old export could verify a guess about content
that was successfully destroyed everywhere else.

With a nonce, `bhash` commits to `nonce ‖ content`, the nonce dies with the body at redaction,
and the commitment becomes hiding. Every property claimed in §1.1 survives: `id` recomputable,
`sig` verifiable, `chain` unbroken. The cost is 16 bytes per event.

**`blake3(canonical body) == bhash` is verified on ingest, always.** `id` and `sig` authenticate
the *hash* of the body, not the body — so without this check a relaying device can substitute a
different body and every downstream replica accepts it: id recomputes, signature verifies, and
two replicas hold permanently different content under the same id, deduplicating each other's
copies on sight. Since events are relayed by design (§5.3), that is precisely the attack per-event
signatures exist to prevent. `capture verify` (§8.6) checks it too. On redacted rows the check is
skipped, because `body IS NULL`.

**`id` must cover the whole envelope, not just the body.** This is load-bearing three separate
ways and getting it wrong breaks all three at once:

- **Authentication.** `sig` covers `id`, so signing the id only authenticates what the id
  commits to. If `id` hashed `body` alone, then `dev`, `seq`, `hlc`, `kind`, `tgt` and `prev`
  would be unsigned — and anyone who had ever seen the log could take a signed body from one
  device and re-emit it as `{dev: <that device>, kind: "edit", tgt: <any note>}`, and every
  replica would verify it. Events are relayed (§5.3), so this is exactly the attack per-event
  signatures exist to stop.
- **Dedup.** `id` is the primary key. Hashing `body` alone would make two identical captures
  collide — "milk" typed twice, or the same `occ_done` from two devices — and the second insert
  would be silently dropped, violating the rule that a capture is *never* rejected. With the
  envelope hashed, `seq` and `hlc` differ, so distinct events are distinct ids.
- **Integrity.** The self-check "does this row hash to its id" covers the routing fields that
  determine what the event *means*, not merely its payload — and, thanks to `bhash`, keeps
  working after redaction.

**`chain`** is the id of the previous event from the same origin, giving each origin a hash
chain. It costs 32 bytes and buys a verifiable notion of "this origin's history up to here",
which is what makes retirement enforceable against a device that lies about its own clock
(§5.3).

`tgt` and `prev` are different on purpose and both are needed:

- **`tgt`** is *which note* — the id of the root `capture`. Stable for the note's whole life,
  set by every event that touches it, and indexed so folding one note is a single indexed
  lookup rather than a walk back up a chain (which would also break on a `leaf` replica that
  holds the edit but not yet the root).
- **`prev`** is *which version you were looking at when you edited*. It exists solely to
  distinguish sequential edits from genuinely concurrent ones (§6). On a `capture`, `tgt` is
  the event's own id and `prev` is null.

`seq` being gap-free per origin is what makes the anti-entropy protocol trivial (§5.5).

`hlc` is a hybrid logical clock: physical milliseconds, a logical counter that increments when
two events land in the same millisecond or when an incoming event is ahead of local time, and
the origin id as final tiebreak. This gives a **total order** that stays human-meaningful
(timestamps still mean roughly what they say) and bounds damage from clock skew.

**Skew is reported, never enforced. Ingest follows the standard HLC receive rule.**

An earlier version of this section said a device *refuses to emit* beyond its own wall clock plus
a tolerance. That rule cannot stand alongside §0 and §2, and building it made the collision
obvious. Adoption is mandatory, so a peer whose clock is ahead — a phone that has not reached an
NTP server this week is enough — really does drag this device's logical clock forward. Refusing
to author in that state closes the capture path on a device that is working perfectly, because of
a clock on a different machine. **Capture always wins.** The lead is measured
(`HlcState::lead_ms`), surfaced as a tier-1 health item in `capture status`, and the event is
written regardless. Ordering can be repaired from a log that contains the note; nothing repairs a
note that was never captured.

- On receive, `l := max(l, m.l)`, then increment the counter. The incoming timestamp is
  **adopted**, not clamped.
- A device **refuses to emit** an event whose HLC would exceed its own wall clock by more than a
  tolerance, and surfaces a peer beyond tolerance as a tier-1 health item (§4.5).

Two earlier drafts got this wrong in opposite directions, and both are worth recording because
each looked like the obvious fix.

**Filtering the fold** (quarantining future-dated events from the projection) made the fold a
function of the local wall clock — three devices with three clocks showing three different states
from the same converged log. It also could not work: the HLC update happens at ingest and the
filter at fold time, so the skew was already absorbed before the filter ran.

**Clamping on ingest** — capping how far an incoming HLC may advance the local clock — destroys
the only guarantee an HLC provides. `l := max(l, m.l)` is *precisely* what makes
`e → f ⟹ hlc(e) < hlc(f)`; capping it deletes causality:

```
desktop wall 12:10 → emits E1 hlc=(12:10:00)      (legal, matches its own clock)
phone   wall 12:00 → ingests E1, clamps, does not adopt
phone   user reads E1 and edits it → E2, prev=E1, hlc=(12:01:00)
⇒ hlc(E2) < hlc(E1), although E2 causally follows E1
```

Every consequence is silent and user-visible: the phone's later correction loses every §6 LWW
race for ten minutes (task state, due dates, `alias`, `promote`, `occ_done`); `notes.updated` does
not advance so the edited note **renders its pre-edit text**; and §6's "fold over the HLC-sorted
union" reaches an edit before the version it supersedes exists. Clamping does not remove the
skew problem, it flips which clock wins — from "fast clock always wins" to "slow clock can never
win."

There is a second trap in the same mechanism: if the emit ceiling and the ingest clamp are the
same quantity, a device that syncs with a faster peer is pinned at its own ceiling and must
either violate the emit rule or **refuse to capture** — breaking the strongest rule in §0.2. With
adopt-on-receive this cannot arise, because the local clock moves with the mesh.

**The fold remains a pure function of the event set. Nothing about the local clock enters it.**

`prev` appears on mutation events and names the exact version being edited. This is what lets
the system distinguish sequential edits from genuinely concurrent ones — see §6.

### Event kinds

| Kind | Meaning |
|---|---|
| `capture` | Raw input. Text, voice blob ref, image, URL. **Never** rejected, never validated. |
| `enrich` | Machine-derived: transcript, title, entities, links, summary. References a capture. |
| `edit` | Human mutation of a note's body. Carries `prev`. |
| `tag` / `untag` | OR-Set membership. `untag` names the specific `tag` event ids it removes. |
| `intent` | An if-then plan: cue spec + action. See §2.4. |
| `fire` | An intent's cue matched and the action was delivered. |
| `review` | A scheduled resurfacing occurred, plus the outcome (**shown / engaged / deferred** — never a graded pass/fail; see §2.7). |
| `tombstone` | Soft delete — hides, does not destroy. Loses to concurrent edits (§6). |
| `redact` | **Destroys content on every replica, permanently.** Sticky and irreversible (§1.1). |
| `promote` | Assigns a derived type — event, task, or recurring series — with its fields (§3.1). Reversible; a later `promote` supersedes it. |
| `occ_move` / `occ_edit` / `occ_cancel` / `occ_done` / `occ_undone` | Single-occurrence exceptions to a recurring series, keyed by `(tgt, occ)` (§3.5). |
| `device_add` | Admits a device: **public key**, role, initial origin. Signed by an existing member (or self-signed as the roster root, §8.9). |
| `origin_claim` | An existing roster key declaring a new origin for itself after a restore (§1). Self-signed; verifiable against the roster key. |
| `device_readmit` | Re-admits a key after a lockout. **Signed by two keys**, valid even if both are retired (§5.3). |
| `device_caps` | A device declaring which enrichment job classes it can run, and whether it can notify (§2.5.2). Signed by that device about itself. Static only — transient state never enters the log. |
| `mute` | Suppresses a note, tag, project, or date range from review and all pushed surfaces (§2.6). Reversible, non-destructive, unlike `redact`. |
| `resolve` | Dismisses a recorded divergence (§6). Needed because `divergences.resolved` is a fold output and cannot be set by local state. |
| `rating` | How an activity occurrence went — mastery and pleasure, both optional (§3.9). A rating of the activity, never of the person. |
| `redact_cancel` | Aborts a queued redaction within its window (§1.1). Eagerly pushed and retained, exactly like `redact`. |
| `alias` | Assigns a chosen human-readable name to a note (§4.3). LWW; reassignment breaks existing links visibly rather than retargeting them. |
| `setting` | A user preference shared by every device — notifier order, thresholds, display. LWW by HLC. **No `setting` may gate destruction or silently suppress delivery** (§5.3); those need their own explicit, confirmed events. |
| `device_retire` | Removes a device. Its past events stay valid; new ones are rejected (§5.3). |

The last two matter more than they look: **the device roster lives in the log itself**, which is
what makes n devices tractable (§5.3).

**Sizing, corrected.** An earlier draft claimed 3.6 MB/year by counting captures alone
(200 bytes × 50/day). That is wrong by one to two orders of magnitude, because captures are a
minority of events:

| Contributor | Scale |
|---|---|
| Envelope overhead before any body | ~200 B — `id` 32, `sig` 64, `tgt` 32, `prev` 32, `chain` 32, `bhash` 32 |
| Seven `enrich` classes per capture (§2.5.1) | 7 events each |
| Transcripts, which live in the event plane | ~10 KB for a 10-minute recording — 50× a text capture |
| `promote` per capture, plus a full reparse sweep whenever the parser improves | 1 per note per sweep |
| `fire`, `review`, `occ_done` per instance | continuous |

Realistically **100–300 MB/year**. Still fine on a phone, and still no compaction needed — but
that is now a claim about a real number rather than one 30–80× too small, and it means event
plane growth is a thing to measure rather than something already dismissed.

---

### 1.1 Redaction — the one thing that really is destroyed

`tombstone` hides. `redact` destroys. Both exist because they answer different needs: "I am done
with this" and "this must not exist."

Some things genuinely have to go — a password pasted into capture, a third party's private
information. An append-only log has to make room for that explicitly, because retrofitting it
later into a content-addressed replicated store is enormously harder than designing for it.

**"Something written in a bad week" is not one of these, and conflating them was a design bug.**
An earlier draft named exactly that case here and offered permanent irreversible destruction as
the only remedy — so a user facing painful material being pushed at them on a schedule they are
forbidden from setting (§0.2) had two options: keep receiving it, or destroy it forever on every
device. That is a bad choice presented at a bad moment, and it is now answered by `mute` (§2.6)
instead. Redaction is for content that must not exist. Muting is for content you do not want to
be shown. They are different needs and only one of them is irreversible.

**Mechanism.** A `redact` event names a target. Every replica that ingests it destroys the
target's `body` and sets a redacted flag. Everything else survives: `id, dev, seq, hlc, kind,
tgt, prev, occ, chain, bhash, sig`. The row cannot be deleted — `seq` must stay gap-free or the
version vector (§5.5) breaks and anti-entropy re-requests the gap forever.

**Verifiability is fully preserved**, which is the reason `id` commits to `bhash` rather than to
the body itself (§1). After redaction the id is still recomputable from the surviving header plus
`bhash`, the signature still verifies against it, and the origin hash chain is unbroken. Nothing
about integrity checking degrades — a redacted row is as verifiable as any other, it simply has
no readable content.

**What "destroys" can and cannot mean on real storage.** Overwriting a value in SQLite does not
reliably erase the prior bytes: the old page image persists in the `-wal` file until checkpoint,
in freelist pages until `VACUUM`, and in SSD flash blocks after that regardless of what the
filesystem believes. Redaction therefore runs a defined sequence — overwrite, checkpoint,
`VACUUM` — and blob redaction overwrites the file before unlinking. Against an adversary with
the raw device this is still best-effort, and on flash it cannot be made a guarantee. Stated
here rather than implied, because "destroyed" invites an assumption the hardware does not honour.

**Sticky, permanently.** This is the part that makes it work. Redactions are retained forever
and applied to *any* matching event that arrives later, from any source. A laptop that was
offline for two weeks has its copy destroyed on ingest; a `leaf` replica that never held the
event still keeps the redaction so a later range-fetch cannot resurrect it. Without stickiness,
anti-entropy would simply undo every redaction the moment a stale peer reconnected.

**Eagerly pushed.** Unlike ordinary events, which rely on anti-entropy and treat push as a
latency optimisation (§5.4), a `redact` is pushed immediately to every known peer and **retried
with backoff until each has acknowledged it**. Here the latency *is* the guarantee: the window
between "I decided this must go" and "it is gone" is the window in which it can still leak.

**Default scope is the whole note.** `redact { tgt: X }` destroys every event targeting X — the
original capture, every superseded edit, enrichments, transcripts, and blob references.
Per-event redaction exists but is not the default, because of a trap worth naming: if you paste
a password and then edit the note to remove it, redacting only the current version leaves the
password sitting intact in the superseded edit event. Whole-note is what people mean.

**Scope is bounded, and destruction is delayed.** Redaction is the only primitive that destroys
data irreversibly on every replica, and as originally specified any single device could enumerate
`tgt` values — they are indexed — and destroy the entire archive everywhere within one push
round. No attacker required; one loop in one buggy build does it.

- **A redact names at most 64 targets**, and — because a loop emits a thousand redacts as easily
  as one — **receiving replicas rate-limit by origin**: beyond 256 targets per hour from a single
  origin, further redactions are **held pending explicit confirmation** rather than executed. The
  bound has to be enforced by the receiver, since the buggy or compromised device is exactly the
  one that will not enforce it on itself. A per-event cap alone bounds nothing.
- **Destruction is queued, not immediate.** Receiving replicas mark the target redacted and hide
  it from every view **on ingest, immediately** — that part is not delayed. Physical destruction
  of the body happens **30 minutes after the redact's own HLC**, not 30 minutes after local receipt.
The length lives in `capture_core::REDACT_WINDOW_MS` and that constant is the authority: this
document said six hours in five places while the code said thirty minutes, which is a twelve-fold
difference in the only safety margin the one irreversible operation has. Corrected here rather
than in the code, because the code is what people were actually relying on and `capture redact`
has always printed the true figure.
  Any device may emit `redact_cancel` within that window.

Deriving the deadline from the redact's HLC rather than local arrival matters: with local timers,
a device that ingests a year-old redaction would grant that plaintext a *fresh* window on a
brand-new machine, forever, on every future pairing. With an HLC-derived deadline, a redact whose
window has already closed destroys on ingest — which is also what §1.1's stickiness requires, and
resolves a contradiction an earlier draft carried between "destroyed on ingest" and "destroyed
after the window closes."

**`redact_cancel` is as sticky and as eagerly delivered as `redact`.** An earlier draft made it an
ordinary fire-and-forget event, which put the safety mechanism on the least reliable channel in
the protocol while the destructive act got the most reliable one. Both are eagerly pushed with
retry (§5.4), both are retained indefinitely, and a cancel arriving after its target has been
destroyed is recorded and reported rather than silently discarded.

**Redaction purges the projection too.** `state.db` holds the FTS5 index over text and transcripts
(§4). Destroying `body` in `events.db` while leaving the search index intact would leave the
redacted password fully searchable on every replica — a complete failure of the stated purpose.

That purge cannot be a single transaction, and an earlier draft claimed it was: `events.db` and
`state.db` are separate SQLite databases in WAL mode, and SQLite provides no atomic commit across
attached databases in WAL. So the order is fixed and durable instead:

```
1. write an intent record to ops.db          (durable, survives crash)
2. purge state.db: projection rows + FTS entries
3. destroy the body in events.db, checkpoint, VACUUM
4. clear the intent
```

A crash between any two steps leaves the intent, and startup replays from it. The order is chosen
so that every interruption leaves the *safe* state — the index is destroyed before the content is,
so no crash window ever leaves searchable text pointing at a body that is already gone.

**Visible, not silent.** A redacted note renders as `[redacted 2026-08-20]` rather than
vanishing. In a system built to compensate for unreliable memory, an unexplained gap is worse
than a marked one — you would spend a year wondering whether the tool lost something. The
marker says: this is missing because you removed it.

**Irreversible, and gated accordingly.** There is no undo. The confirmation names exactly what
is about to be destroyed — "3 notes and 1 audio recording, permanently, on every device" — and
this is the only destructive confirmation in the system, so it should not be trained away by
appearing routinely.

**The honest guarantee** is *removed from every live replica and every projection that syncs
after the redact*. The limits, stated in full rather than the two I originally admitted:

1. **A device that never reconnects keeps its copy.** No peer-to-peer design can do better.
2. **A returning stale replica can briefly re-serve the original** to a third peer before
   receiving the redact; that copy is destroyed a round later.
3. **Exports are outside the reach of redaction.** §1.2 writes plaintext events to a directory
   that external tools are explicitly encouraged to copy offsite — deduplicated, versioned,
   deliberately immutable snapshots. A `redact` cannot touch any of them. The exporter therefore
   applies retained redactions when writing *new* export files, and the UI states plainly that
   existing exports and any offsite copies must be pruned by hand. This is a genuine hole and
   naming it is the only honest option.
4. **Physical destruction is best-effort on flash storage**, per the mechanism note above.
5. **Text that other notes copied is not reachable.** If note B links to note A, B's own text
   contains A's slug — and a slug is derived from A's opening words (§4.3). Redacting A destroys
   A; it does not touch B's text, B's `links` row, or B's search index entry. §4.3 therefore
   forbids deriving slugs from sensitive-looking captures and provides `redact --with-links`,
   which redacts inbound linking text alongside the target. Neither fully closes the hole:
   anything a human copied by hand into another note is beyond reach by construction.

If the requirement is "this must not exist anywhere, including in backups," redaction alone does
not deliver it and no feature in this design does.

### 1.2 Export — because replication is not backup

n replicas protect against losing a device and against nothing else. The design actively works
against you here: every replica faithfully replicates whatever the log says, so a client that
appends malformed events propagates them everywhere within seconds, by design. Gossip makes
corruption travel *faster*, not slower.

| Threat | Covered by replication? |
|---|---|
| Device dies, is stolen, or is reinstalled | yes |
| A bug appends wrong or malformed events | **no** — replicated everywhere |
| A bad projection or migration | **no** |
| A `redact` fired at the wrong note | **no** — irreversible by design (§1.1) |
| Every device lost together (fire, theft, one bag) | **no** |

The last two matter most. Redaction is deliberately permanent and eagerly pushed, so a misfire
is unrecoverable without an out-of-band copy. And "all devices at once" is not exotic — the
laptop and the phone are usually in the same bag.

**Shape: incremental event export.** The log is append-only and immutable, which makes this
unusually simple — a snapshot can never be internally inconsistent, there is nothing to quiesce
or lock, and "what is new" is exactly *everything after `seq` N per origin*. The exporter writes
new events since its last run into a dated file in an export directory. Restore is a replay:
ingest the files, re-fold, done. No coordination and no special path — restore uses the same
ingest code as sync, so it is exercised constantly rather than being the untested branch that
fails on the one day it matters.

**Restore increments the incarnation** before the first local append (§1). Skipping that step is
the silent-data-loss failure described there, and it is the one part of restore that must not be
left to operator discipline — the restore tool does it automatically.

**Restore and redaction pull in opposite directions, deliberately.** Sticky redactions apply to
events arriving from *any* source (§1.1), which includes a replayed export. That is correct for
the ordinary case: restoring a two-year-old backup must not resurrect a password you destroyed
last month. But it also means a plain restore cannot recover from a *misfired* redact, since the
redaction destroys the restored copy on arrival — and §1.2 exists partly to cover that case.

Resolved by making the two intents distinct operations rather than hoping one covers both:

| Operation | Redactions applied | Use |
|---|---|---|
| `restore` | yes, all of them | Ordinary recovery. Disk died, device replaced. |
| `restore --ignore-redactions-after T` | only those issued at or before T | Deliberate undo of a redact you regret, from a backup predating it. |

The second is a conscious, explicitly-flagged act with a stated timestamp, not a default. The
destruction window and the pre-redact snapshot below both cover the common case; this
exists for the case where you notice a month later.

**Offsite is out of scope.** Point restic, borg, rsync or a copy job at the export directory.
Rebuilding that badly is a poor use of effort when good tools already exist.

**A designated archive peer is not a backup.** A NAS or Pi joined as a permanent `full` replica
is useful for availability, but it replicates bad events and honours a wrong `redact` exactly
like every other replica. Worth stating because it is a tempting and plausible-looking
substitute for the real thing.

**Pre-redact snapshot: on, 30-day expiry.** Issuing a `redact` first writes a local snapshot of
the affected notes, which expires automatically. This plugs the hole redaction opens — a misfire
is recoverable during the window in which you would notice it — while leaving redaction itself
genuinely permanent in the log and on every replica.

The tension is real and worth naming rather than glossing: the snapshot is by definition a copy
of content just declared as must-not-exist. It is defaulted **on** because the realistic case is
"I do not want to see this again," not "someone will find this." A future panic redact (§9.14)
is the escape hatch for the latter and skips the snapshot deliberately. If the threat model ever
changes, this default is the first thing to revisit — see §9.14, where it interacts badly with
at-rest encryption.

### 1.3 Schema evolution — the log never migrates

Three devices update on different schedules; a phone can be two versions behind a desktop for a
month. Meanwhile `events.db` has to stay readable in ten years while body shapes change — a
`promote` gains a field, a cue type is added, `occ_undone` appears.

**The governing rule: the log never migrates. Only the projection does, and projection migration
is `DROP` and re-fold.** `state.db` is disposable by construction (§7 mode 12), so a schema change
means bumping the projection version, noticing the mismatch on startup, and re-folding. There is
no `ALTER TABLE`, no version-N-to-N+1 script, and **no migration code, ever** — which is worth
more over a decade than any amount of careful migration tooling.

**Forward compatibility is absolute.** A device never refuses to sync with a peer because that
peer is too new or too old. There is no minimum-version floor. A device two years behind is still
a full participant in replication; it simply cannot *display* what it does not yet understand.

Five rules make that work:

1. **Fields may be added, never removed or repurposed.** Unknown fields are ignored by older
   readers and preserved verbatim, since events are immutable and never rewritten.

2. **The semantics of an existing kind never change.** A breaking change is a **new kind name**,
   not a new meaning for an old one. This is the rule that prevents two client versions from
   silently disagreeing about what history says — the worst available failure, because nothing
   surfaces it. Kind names are cheap; reinterpreted history is not.

3. **Unknown or malformed events are quarantined, not dropped** — *after* their signature
   verifies. Stored at their position, `heads` may advance, excluded from the fold (§5.3).
   Dropping them is not an option: `seq` must stay gap-free or anti-entropy re-requests the hole
   forever. Events whose signature does **not** verify are a different case entirely and never
   occupy a position (§5.3).

   Because a malformed event must still be storable, the `events` row carries the envelope
   fields only as far as they parse: a body that fails to decode is stored as opaque bytes with
   `kind` set to the literal `?`, and the `STRICT` schema is satisfied because every column that
   is `NOT NULL` — `id`, `dev`, `seq`, `hlc`, `bhash`, `sig` — is part of the *signed header*
   and therefore known-good before the body is examined. An event malformed in those fields
   cannot have verified its signature, so it is rejected rather than quarantined and the
   question does not arise.

4. **Relay does not require comprehension.** An old phone must still pass a new event kind from
   the desktop to the laptop. Signature verification operates on the envelope and needs nothing
   from the body, so this works — but it must be stated, because "I do not understand this, I
   will drop it" is the intuitive wrong move and would partition the mesh by software version.

5. **The canonical encoding is frozen permanently.** Deterministic CBOR with explicit field
   ordering, specified once. `id` is a hash over that encoding, so any change to it would change
   every id in the log and fork the archive. An `envelope_version` field exists for the rare case
   where the envelope shape itself must change; altering it is a deliberate, converted fork, not
   something that happens by upgrading a dependency.

**Upgrades unquarantine.** When a client learns a kind it previously could not interpret, those
events leave quarantine and fold normally on the next re-fold. Nothing was lost while it waited —
it was replicated, verified, and relayed the whole time.

**Cost, stated:** re-folding is O(n) over the log, so at the sizing in §1 a decade is 1–3 GB and
a re-fold takes minutes rather than milliseconds. It runs in the background into a new `state.db`
while the existing one stays readable, then swaps. Never a blocking upgrade; the price is
transient double disk usage.

### 1.4 The projection

`events.db` is truth. `state.db` is a cache of it, shaped for reading, and disposable —
everything here is reconstructed by folding the log (§1.3).

**Hybrid, not fully materialised.** Materialise what is read constantly; derive the rest on
demand. The reason is failure mode rather than performance: full materialisation spreads fold
logic across every writer, and a bug there leaves rows that are *silently wrong but look
authoritative*. In a system built to compensate for unreliable memory, confidently-wrong is the
worst available failure. Keeping subtle things derived means the log stays the single answer.

**The fold takes the event set and nothing else** (§8.2). Anything that depends on the current
time is therefore **not** a column here — it is computed at read time. That excludes two things
that look like they belong: an occurrence being *lapsed* (it is simply pending with a past
start), and trailing completion **rates** (§3.6), which are queries over `occurrences`, not
stored values. Folding either would make two devices disagree about the same log.

```sql
-- ---------- identity of the projection itself ----------
CREATE TABLE meta (k TEXT PRIMARY KEY, v TEXT NOT NULL) STRICT;   -- schema_version, slug_version

-- Incremental fold watermark, PER ORIGIN. A single scalar cannot work: events arrive
-- out of HLC order by construction (a phone reconnecting delivers week-old timestamps),
-- so one watermark either skips every late arrival or forces a full re-fold on every
-- reconnection — which §1.3 prices at minutes over a decade.
CREATE TABLE fold_head (dev TEXT PRIMARY KEY, seq INTEGER NOT NULL) STRICT;
-- Folding is incremental per `tgt`, using events_tgt(tgt, hlc); a late arrival re-folds
-- only the notes it touches.

-- ---------- roster: needed by INGEST, not just by views ----------
-- Every event is verified against its origin's key on ingest (§5.3). Without this
-- materialised, ingest would scan events.db for device_add/device_retire per event.
-- It lives here and is rebuilt by the fold like everything else, but ingest depends on
-- it — so a re-fold builds the roster first, and ingest pauses rather than accepting
-- unverified events while it is unavailable.
CREATE TABLE roster (
  pubkey     BLOB PRIMARY KEY,      -- membership is key-keyed, not origin-keyed (§5.3)
  role       TEXT NOT NULL,         -- full | leaf
  retired    INTEGER NOT NULL DEFAULT 0,
  can_notify INTEGER NOT NULL DEFAULT 0
) STRICT;
CREATE TABLE roster_origins (
  dev        TEXT PRIMARY KEY,      -- H(pubkey ‖ incarnation)
  pubkey     BLOB NOT NULL REFERENCES roster(pubkey),
  retire_head INTEGER,              -- last legitimate seq, NULL if not retired (§5.3)
  retire_id  BLOB
) STRICT;
CREATE TABLE device_caps (dev TEXT NOT NULL, job_class TEXT NOT NULL,
                          PRIMARY KEY (dev, job_class)) STRICT;   -- §2.5.2

-- Leaf replicas hold a suffix with holes, which one integer cannot describe (§5.5).
CREATE TABLE held_ranges (dev TEXT NOT NULL, lo INTEGER NOT NULL, hi INTEGER NOT NULL,
                          PRIMARY KEY (dev, lo)) STRICT;

-- ---------- settings, with provenance ----------
CREATE TABLE settings (k TEXT PRIMARY KEY, v TEXT NOT NULL, set_by TEXT, hlc BLOB) STRICT;
-- Absent key = compiled-in default. §8.9 requires `capture status` to distinguish
-- "default" from "set by you", which needs the row to be absent rather than seeded.

-- ---------- intents (§2.4) ----------
CREATE TABLE intents (
  tgt      BLOB PRIMARY KEY REFERENCES notes(tgt),
  cue_type TEXT NOT NULL,           -- time | recurring | location | app_focus | …
  cue      BLOB NOT NULL,
  action   TEXT NOT NULL,
  active   INTEGER NOT NULL DEFAULT 1
) STRICT;
CREATE INDEX intents_cue ON intents(cue_type) WHERE active = 1;   -- hot path on every focus change
CREATE TABLE fires (tgt BLOB NOT NULL, occ INTEGER NOT NULL, dev TEXT NOT NULL,
                    PRIMARY KEY (tgt, occ)) STRICT;               -- §2.4.1 dedup

-- ---------- blobs and enrichment ----------
CREATE TABLE blob_refs (hash BLOB NOT NULL, tgt BLOB NOT NULL,
                        PRIMARY KEY (hash, tgt)) STRICT;          -- refcount = count(*)
CREATE TABLE enrich_queue (tgt BLOB NOT NULL, job_class TEXT NOT NULL, rank INTEGER,
                           due INTEGER, PRIMARY KEY (tgt, job_class)) STRICT;

-- ---------- redaction, derived (see below for why it is not in ops.db) ----------
CREATE TABLE redact_queue (redact_evt BLOB PRIMARY KEY, tgt BLOB NOT NULL,
                           execute_after INTEGER NOT NULL,   -- from the redact's HLC, not receipt
                           cancelled INTEGER NOT NULL DEFAULT 0) STRICT;

-- ---------- activation (§3.9) ----------
CREATE TABLE ratings (
  tgt      BLOB NOT NULL,           -- the activity
  occ      INTEGER NOT NULL,        -- which occurrence
  mastery  INTEGER,                 -- 0-4, NULL = skipped, which is missing data not a failure
  pleasure INTEGER,
  PRIMARY KEY (tgt, occ)
) STRICT;
-- No aggregate table by design. There is no wellness score and no mood series; the only
-- derived view is activity-to-day correlation, which is a statement about activities.

-- ---------- coarse mute (§2.6) ----------
CREATE TABLE mutes (scope TEXT NOT NULL, value TEXT NOT NULL, until INTEGER,
                    PRIMARY KEY (scope, value)) STRICT;          -- note | tag | entity | daterange

-- ---------- notes: the head state of every note ----------
CREATE TABLE notes (
  tgt        BLOB PRIMARY KEY,      -- root capture id = note identity
  slug       TEXT NOT NULL UNIQUE,  -- human-readable canonical name, derived + immutable (§4.3)
  alias      TEXT UNIQUE,           -- optional human-chosen name; NULL for most notes
  type       TEXT NOT NULL,         -- note | task | event | series   (§3.1)
  text       BLOB,                  -- captured text, verbatim. NULL if redacted.
  descriptor TEXT,                  -- rendered one-line summary of structured fields (§3.1.4)
  head       BLOB NOT NULL,         -- event id of the winning current version
  created    BLOB NOT NULL,         -- hlc of the root capture
  updated    BLOB NOT NULL,         -- hlc of head
  muted_until INTEGER,              -- NULL = not muted; 0 = muted indefinitely (§2.6)
  pinned     INTEGER NOT NULL DEFAULT 0,
  redacted_at INTEGER,              -- NULL = not redacted. A date, because §4.2 renders one.
  diverged   INTEGER NOT NULL DEFAULT 0,
  tz         TEXT                   -- capture-time IANA zone, so day grouping survives travel
) STRICT;
CREATE INDEX notes_updated ON notes(updated DESC);      -- timeline view
CREATE INDEX notes_type    ON notes(type, updated DESC);

-- ---------- the version graph: edges, not just heads ----------
-- Required by §6: concurrency detection is reachability through `prev`, which needs
-- edges to walk. Without this table that rule is unimplementable — which is exactly
-- how it came to be specified incorrectly in an earlier draft.
CREATE TABLE note_versions (
  event_id    BLOB PRIMARY KEY,
  tgt         BLOB NOT NULL,
  prev        BLOB,                 -- NULL for the root capture
  hlc         BLOB NOT NULL,
  dev         TEXT NOT NULL,
  kind        TEXT NOT NULL,        -- §6 applies different rules to edit / promote / tombstone
  quarantined INTEGER NOT NULL DEFAULT 0
) STRICT;
-- Quarantined events keep their EDGE here even though they are excluded from the fold.
-- Otherwise a `prev` pointing at a quarantined event dangles, the walk terminates early,
-- and legitimate sequential edits are reported as divergences — the exact "wrong in both
-- directions" failure the reachability rule was written to eliminate. Edges are cheap;
-- a wrong divergence count destroys trust (§3.1.3).
CREATE INDEX nv_tgt  ON note_versions(tgt, hlc);
CREATE INDEX nv_prev ON note_versions(prev);            -- walk forward from a version

CREATE TABLE divergences (
  tgt      BLOB NOT NULL,
  branch_a BLOB NOT NULL,           -- competing head
  branch_b BLOB NOT NULL,
  resolved INTEGER NOT NULL DEFAULT 0,   -- set by a `resolve` event; see below
  PRIMARY KEY (tgt, branch_a, branch_b)
) STRICT;
-- `resolved` is a fold output, so dismissing a divergence must AUTHOR an event —
-- otherwise it is either never clearable (and tier 2 reports it forever) or it is local
-- state in the wrong database, resurrected by every re-fold. Hence the `resolve` kind.

-- ---------- structured types ----------
CREATE TABLE tasks (
  tgt       BLOB PRIMARY KEY REFERENCES notes(tgt),
  state     TEXT NOT NULL,          -- open | done
  due_date  TEXT,                   -- floating YYYY-MM-DD, never an instant (§3.2)
  sort      REAL
) STRICT;

-- One-off timed and all-day events. An earlier draft had only `series` + `occurrences`,
-- with occurrences.series_tgt NOT NULL — so a dentist appointment had nowhere to live and
-- the calendar, agenda and `capture cal` had no data behind them.
CREATE TABLE events_at (
  tgt        BLOB PRIMARY KEY REFERENCES notes(tgt),
  start_utc  INTEGER,               -- timed: instant
  end_utc    INTEGER,
  start_date TEXT,                  -- all-day: floating date, never converted (§3.2)
  end_date   TEXT,
  all_day    INTEGER NOT NULL DEFAULT 0,
  tz         TEXT,                  -- required for timed events
  location   TEXT
) STRICT;
CREATE INDEX events_at_start ON events_at(start_utc);
CREATE INDEX events_at_date  ON events_at(start_date);

CREATE TABLE series (
  tgt             BLOB PRIMARY KEY REFERENCES notes(tgt),
  rrule           TEXT NOT NULL,
  tz              TEXT NOT NULL,    -- IANA id; required to expand across DST (§3.2)
  dtstart         INTEGER NOT NULL,
  until           INTEGER,          -- set when a split bounds this series (§3.5)
  all_day         INTEGER NOT NULL DEFAULT 0,
  carries_forward INTEGER NOT NULL DEFAULT 0,   -- §3.6 — medication, invoices
  split_from      BLOB
) STRICT;

-- Expanded lazily over a rolling ±1y horizon. Pure projection: rebuildable, and
-- re-derived rather than migrated when the tz database changes (§7 mode 21).
CREATE TABLE occurrences (
  series_tgt     BLOB NOT NULL REFERENCES series(tgt),
  original_start INTEGER NOT NULL,  -- the occurrence's stable identity (§3.5)
  start_utc      INTEGER,           -- NULL for all-day
  start_date     TEXT,              -- floating date for all-day
  end_utc        INTEGER,
  all_day        INTEGER NOT NULL DEFAULT 0,    -- mirrored so the calendar need not join
  state          TEXT NOT NULL,     -- pending | done | cancelled | moved
  title          TEXT,              -- occ_edit overrides, this occurrence only (§3.5)
  location       TEXT,              -- occ_edit
  note           TEXT,              -- occ_edit
  dst_shifted    INTEGER NOT NULL DEFAULT 0,    -- surfaced in the calendar, never silent
  PRIMARY KEY (series_tgt, original_start)
) STRICT;
CREATE INDEX occ_start ON occurrences(start_utc);
-- "lapsed" is deliberately absent: it is `state='pending' AND start < now`, and now
-- must never enter the fold (§8.2).

-- ---------- organisation ----------
CREATE TABLE tags (                 -- live OR-Set membership (§6)
  tgt      BLOB NOT NULL,
  tag      TEXT NOT NULL,
  add_evt  BLOB NOT NULL,           -- the specific add an untag must name to remove
  PRIMARY KEY (tgt, tag, add_evt)
) STRICT;

-- Entities have identity of their own, so that renaming a person does not fork their
-- page and two people named Sarah are distinguishable. `mentions` is the extraction.
CREATE TABLE entities (
  ent_id     BLOB PRIMARY KEY,      -- stable; survives renaming
  kind       TEXT NOT NULL,         -- person | project | client
  display    TEXT NOT NULL,
  note_tgt   BLOB,                  -- optional backing note (§4.3)
  merged_into BLOB                  -- set when two entities are merged; resolution follows it
) STRICT;
CREATE TABLE mentions (tgt BLOB NOT NULL, ent_id BLOB NOT NULL,
                       PRIMARY KEY (tgt, ent_id)) STRICT;
CREATE INDEX mentions_ent ON mentions(ent_id);

CREATE TABLE ambient (              -- the retrieval scaffolding from §2.3
  tgt BLOB PRIMARY KEY, app TEXT, window_title TEXT, repo TEXT, branch TEXT,
  cal_event TEXT, source_url TEXT, location TEXT
) STRICT;
CREATE INDEX ambient_repo ON ambient(repo, branch);      -- context browse (§4)

CREATE TABLE collections (name TEXT PRIMARY KEY, query TEXT NOT NULL) STRICT;

-- ---------- link graph (§4.3) ----------
CREATE TABLE links (
  src       BLOB NOT NULL,          -- note containing the link
  raw       TEXT NOT NULL,          -- the slug/alias as written
  bound_tgt BLOB,                   -- what it resolved to WHEN AUTHORED (§4.3)
  dst       BLOB,                   -- what it resolves to NOW; NULL = broken
  kind      TEXT NOT NULL,          -- explicit | machine
  weight    REAL,                   -- machine links only: similarity
  PRIMARY KEY (src, raw, kind)
) STRICT;
CREATE INDEX links_dst ON links(dst, kind);        -- backlinks, in one query
-- bound_tgt <> dst  =>  render broken, never silently retarget

-- ---------- enrichment ----------
CREATE TABLE enrichments (
  tgt       BLOB NOT NULL,
  job_class TEXT NOT NULL,          -- title | entities | links | transcribe | …
  event_id  BLOB NOT NULL,
  rank      INTEGER NOT NULL,       -- frozen rendezvous rank; lower wins (§2.5.3)
  result    BLOB,
  PRIMARY KEY (tgt, job_class)
) STRICT;

-- ---------- search ----------
-- FTS5 rowids are integers; note identity is a 32-byte BLOB. Without this mapping a
-- search hit cannot be resolved back to a note, and a specific note's index row cannot
-- be deleted — which redaction requires.
CREATE TABLE fts_map (rowid INTEGER PRIMARY KEY AUTOINCREMENT, tgt BLOB NOT NULL UNIQUE) STRICT;

CREATE VIRTUAL TABLE note_fts USING fts5(
  text, title, transcript,
  content='', contentless_delete=1     -- deletion REQUIRES this; plain contentless cannot delete
);
-- Redaction deletes from here before destroying the body, in the ordered sequence of §1.1
-- (cross-database atomicity is not available). Leaving the index intact would keep a
-- redacted password fully searchable on every replica.
-- Indexed text is the CURRENT head, not the root capture — otherwise nothing added in an
-- edit would ever be findable. Redaction therefore purges every version's contribution.

-- ---------- derivable operational state ----------
-- Authentic but unfoldable. These DO occupy their (dev, seq) and may advance heads.
CREATE TABLE quarantine (
  event_id BLOB PRIMARY KEY,
  reason   TEXT NOT NULL   -- unknown_kind | malformed | not_admitted | post_retire | chain_gap
) STRICT;
```

There is deliberately no `clock` reason. Clock skew is reported as a health item and absorbed by
the HLC receive rule (§1); it never blocks authoring and never filters the fold, because filtering
would make the projection a function of the local wall clock and the same log would render
differently on three machines.

**Unauthenticated events live outside `events.db` entirely**, in `ops.db`, keyed by `id` with no
`(dev, seq)` claim and no effect on `heads`:

```sql
-- ops.db
CREATE TABLE rejected (id BLOB PRIMARY KEY, dev TEXT, seq INTEGER, reason TEXT NOT NULL,
                       first_seen INTEGER NOT NULL, raw BLOB) STRICT;  -- bad_sig | bhash_mismatch
```

Kept for forensics, capped by size, never relayed, never folded, and — critically — never
occupying a position a genuine event may later need.

**Local operational state lives in a third store, `ops.db`, which is never synced and never
re-folded.** The split is by derivability, not by category: anything reconstructible from the log
belongs in `state.db` and is rebuilt for free, while state that exists only on this device and
cannot be recomputed must survive a re-fold. Wiping it would be a real bug — a re-fold must not
clear the timer on a queued redaction.

```sql
-- ops.db — local only, survives re-fold, never replicated
CREATE TABLE redact_intent (redact_evt BLOB PRIMARY KEY, phase TEXT NOT NULL) STRICT;
                           -- crash-recovery marker for the ordered purge (§1.1)
CREATE TABLE redact_acks  (redact_evt BLOB NOT NULL, peer TEXT NOT NULL, acked INTEGER,
                           PRIMARY KEY (redact_evt, peer)) STRICT;   -- pinned at issue (§5.4)
CREATE TABLE rejected     (id BLOB PRIMARY KEY, dev TEXT, seq INTEGER, reason TEXT NOT NULL,
                           first_seen INTEGER NOT NULL, raw BLOB) STRICT;   -- §5.3
CREATE TABLE peers        (origin TEXT PRIMARY KEY, last_addr TEXT, last_seen INTEGER,
                           last_vv_match INTEGER) STRICT;            -- staleness for §4.5
CREATE TABLE pairings     (peer_key BLOB PRIMARY KEY, paired_at INTEGER NOT NULL) STRICT;
                           -- so "unexpected device_add" is computable (§4.5)
-- NOTE: the capture spool is NOT a table. It is a plain 10 MB preallocated file at
-- ~/capture/spool, because a SQLite table would need the same disk that just filled up.
-- See §7 mode 6.
CREATE TABLE blob_local   (hash BLOB PRIMARY KEY, bytes INTEGER, last_used INTEGER) STRICT;
CREATE TABLE cal_mirror   (cal_id TEXT NOT NULL, uid TEXT NOT NULL, start_utc INTEGER,
                           end_utc INTEGER, start_date TEXT, all_day INTEGER, title TEXT,
                           synced_at INTEGER, PRIMARY KEY (cal_id, uid)) STRICT;
                           -- §3.9: external calendars are never in the log, so not in state.db
```

**The redaction queue is *not* here.** It is fully derivable — the `redact` event, its HLC, and
any `redact_cancel` — so it belongs in `state.db` and is rebuilt by the fold. Putting it in
`ops.db` would mean a disk-death restore replays `events.db`, marks notes redacted, purges the
projection, and then **never destroys the bodies**, because nothing drives the destruction. The
restored replica would hold every redacted plaintext indefinitely while the status page reported
all redactions delivered. Only the crash-recovery marker and the per-peer ack set are genuinely
local.

The capture spool lives outside all three databases as a plain file; `seq` is assigned when it
drains, not when it fills, so a partial drain cannot leave a gap (§7 mode 6).

---

## 2. Capture

### 2.1 The verb

Capture is a **single verb with no arguments**. There is no "which notebook," no "what type,"
no title field, no tag picker. You press the key, you type or speak, it commits.

Everything a normal app asks you at capture time is deferred to the enrichment pipeline (§2.3),
which runs later, on the desktop, without you.

Latency target: **hotkey → blinking cursor in under 150 ms**, achieved by keeping a resident
process with a pre-warmed, pre-laid-out window that is shown rather than constructed.

**This number is a UI responsiveness convention, not a research finding** — nothing in
`EVIDENCE.md` yields a latency threshold, and an earlier draft presented it as though the
evidence demanded it. Two honest qualifications:

- §2.9 puts the real interaction cost at about a second. Window-show is a fraction of that, so
  this is not where the leverage is.
- ACT (finding 6) predicts the *opposite* of "if it is slower than the thought, you stop using
  it" for an anxious user: they are more likely to keep using it and absorb the cost silently.

It stays as a target because a capture surface that feels sluggish is a capture surface you
resent, and that is worth avoiding on ordinary craft grounds. It should not be used to justify
large architectural commitments on its own — see §8.4, where it does exactly that.

Commit is `Enter`. Discard is `Esc`. There is no third option.

### 2.2 Surfaces

The gesture is identical on desktop and laptop — same chord, same overlay, same dimensions, same
commit key. Context stability is among the strongest accelerators of automaticity in the habit
literature, and cross-device inconsistency works against it.

**Honest qualification:** the list below is seven surfaces, not one. §0 states the rule as
"gestures are as similar as each platform allows" rather than "byte-identical", because the
latter is contradicted by the very next paragraph — a Quick Settings tile is not a keyboard
chord, and no amount of design intent makes it one. What *is* achievable, and what matters, is
that the desktop and laptop surfaces are genuinely identical, and that each platform has **one
primary** gesture that is always the same on that platform. The alternates exist because the
primary one is not always reachable; they are not equal citizens.

**Linux desktop / Mac laptop** — two primary surfaces, not one and a fallback:

- **Terminal keybinding** (§8.6). Type the text, press the chord; the raw line is captured with the
  shell's working directory, git repository and branch attached.
- **Global hotkey → overlay.** Single text field, for capturing from anywhere else.
- Hold-to-talk variant on the overlay chord for voice.

An earlier draft framed the terminal path as the thing you use "when a terminal is already
focused", i.e. a convenience. On the target machine it is the **better** surface, because it is the
only one that can collect context at all (§2.3): Wayland gives an overlay no way to learn what you
were doing. Promoting it is a consequence of what the platform actually permits rather than a
preference.

This does mean two chords on one platform, which sits awkwardly against the context-stability rule
in §0.2. The mitigation is that they are chords in *different places* — one inside a terminal, one
everywhere else — so neither competes with the other for the same muscle memory, and the difference
in what gets captured is visible in the timeline rather than hidden.

**Android phone**
- Quick Settings tile (one pull, one tap — reachable from the lock screen).
- Share-sheet target (captures from any app, with source URL attached automatically).
- Assistant voice intent.
- Long-press home → hold-to-talk.

Voice on the phone records locally and commits immediately as a blob reference. It does **not**
transcribe on device. Transcription happens on the desktop when the blob arrives, and comes
back as an `enrich` event. The phone's job ends at "the audio exists."

### 2.3 The ambient envelope

This is the part that addresses the encoding deficit directly.

At capture time, at zero cost to you, the client attaches whatever context it can observe:

- wall time, device, and HLC
- foreground application and window title
- current working directory, git repo, and branch, if any
- calendar event in progress, if the calendar is readable
- source URL and page title, for share-sheet captures
- coarse location, on the phone only, and only if enabled
- the last clipboard entry, if it changed within the previous 60 seconds

None of this is asked for. All of it is **retrieval scaffolding** — where, when, what you were
doing — attached automatically so that an otherwise useless note ("budget 40k, revisit") is
recoverable three weeks later. Because retrieval is the intact faculty (§0.3), giving it better
cues is leaning on a strength.

**What is actually obtainable, checked on the target machine (GNOME 46, Wayland):**

| Field | Terminal capture | Overlay capture |
|---|---|---|
| timestamp, timezone, device | yes | yes |
| working directory | yes — it is the CLI's own | **no** |
| git repository and branch | yes | **no** |
| foreground application, window title | n/a | **no** |

Wayland exposes no protocol for focused-window identity, `org.gnome.Shell.Eval` is disabled in
release builds, and `xprop -root _NET_ACTIVE_WINDOW` under Xwayland returns `0x0` — a
plausible-looking answer that means nothing, so it must be explicitly refused rather than read.
Without the focused window there is no PID, and therefore no `/proc/<pid>/cwd` either.

**The consequence is asymmetric and worth stating plainly rather than discovering later:** notes
captured from the terminal carry the context that makes context-browse (§4.1) work; notes captured
from the overlay largely do not. That is a genuine reduction in what §4.1 calls the distinctive
browse axis, on the machine where most work happens.

The usage rule that falls out: **capture from the terminal when the note belongs to what you are
working on; capture from the overlay when you are anywhere else.** A note without context simply
renders without a context strip, which the timeline already handles — so the difference is visible
rather than mysterious.

**The clipboard is deliberately excluded.** An earlier draft listed "the last clipboard entry, if
it changed within the previous 60 seconds", and `wl-paste` makes it technically available. It is
left out because silently copying the clipboard into an append-only log is precisely how a password
gets in there — which is the case redaction exists for, and redaction cannot reach exports. The
convenience is not worth creating the problem the design already has to work around.

An earlier draft claimed this "directly counteracts the overgeneral-memory pattern." **That
claim is withdrawn.** OGM is a property of how a person searches their own memory, not of how
much metadata sits in a file. Storing specific detail externally does not train specific
retrieval — if anything it substitutes for the retrieval practice that is the active ingredient
in c-MeST. The ambient envelope makes notes findable. It does not treat anything.

### 2.4 Intents (if-then)

An `intent` event is a first-class if-then plan. An earlier draft justified that by calling it
"the single best-evidenced ADHD-specific intervention in the material reviewed" — a claim §0.1
finding 3 already establishes is false on three counts, and which survived here for 800 lines
after §0 was corrected. It is **not ADHD-specific** (a general child meta-analysis), **not
best-evidenced** (g = 0.31 is smaller than c-MeST, the pooled digital-intervention effect, and
behavioural activation), and **has not been studied in adults with ADHD at all**.

The honest justification is weaker and still sufficient: if-then plans have a small effect with
consistent direction, they appear as a component of CBT for adult ADHD, and — unlike most things
here — the mechanism is cheap to implement and easy to abandon if it does no good. That is a
reason to build it, not a reason to call it the best thing available.

```jsonc
{ "kind": "intent",
  "cue":  { "type": "app_focus", "match": "Slack" },
  "then": "check the #incidents backlog before anything else" }
```

Supported cue types, each cheap to observe on at least one device:

| Cue | Observed by |
|---|---|
| `time` / `recurring` | any device |
| `location` (geofence) | phone |
| `app_focus` | desktop, laptop |
| `calendar_start` | any device with calendar read |
| `device_wake` | any device |
| `git_branch` | desktop, laptop |

#### 2.4.1 Which device fires

Unlike enrichment (§2.5.3), this is **not** an assignment problem and deliberately does not use
rendezvous. Enrichment devices are interchangeable — any of them produces the same transcript,
so the only goal is avoiding duplicate work. Notification devices are not interchangeable: a
desktop toast at 3pm while you are out is worthless no matter how fairly it was assigned. The
question is *where will you actually see this*, which is a preference, not a load-balancing
decision.

So: **you choose, in the UI.** Devices announce whether they can notify — permission granted,
notifications enabled, currently reachable — and you pick the primary. In practice this is the
phone.

Your choice is a `setting` event, so it converges to every device like anything else.

**Until you choose, the notifier follows a compiled-in default rule** — "the most recently paired
device that can notify" — evaluated locally and never written to the log. An earlier draft had the
first Android device paired *emit a `setting` on your behalf*, which is precisely what §8.9
forbids: a machine-authored preference recorded as a user decision, that `capture status` would then
report as "set by you" and that could never be centrally improved. Defaults are rules; settings
are decisions; conflating them makes the distinction §8.9 exists to preserve unrecoverable.

**Local cues override the designation, always.** Some cues are only observable where they
happen, and moving them would destroy the point of §2.4:

| Cue | Fires on |
|---|---|
| `location` (geofence) | the phone — nothing else can see it |
| `app_focus`, `git_branch` | the machine running the app — firing "check #incidents when you open Slack" on your phone is useless when you are sitting at the desktop |
| `time`, `recurring`, `calendar_start`, `device_wake` | the designated notifier |

Only the last row is a choice. The rest fire where they are observable, because
point-of-performance delivery is the entire mechanism — a reminder that arrives somewhere you
are not is just a notification.

**Fallback is an ordered list, not a single primary.** Phone, then laptop, then desktop. If the
primary has not been seen by a device for longer than a grace window, the next in the list
fires. Before firing, a fallback device waits a short jittered delay and re-checks for a `fire`
event, which removes most duplicates cheaply.

**Duplicates are the acceptable failure, and this is a deliberate ordering.** A partition — the
phone on cellular at the office, the desktop at home concluding the phone is gone — produces two
notifications. That is fine. **A duplicate nudge is much better than a missed one**, because a
missed intent silently fails at exactly the thing the intent existed to do, and you never find
out. This priority is stated so that implementation cannot quietly invert it in the name of
tidiness.

Measurement stays correct regardless: both devices emit `fire` events, so the log records the
duplicate honestly, and the projection counts the *occurrence* as fired once when scoring
whether the plan is working.

Notification content is redacted on the lock screen by default — "Brain: 1 reminder", expanding
only once the device is unlocked. Point-of-performance delivery otherwise means your reminders
are legible on a locked phone in a stranger's hand (§9.14).

When a cue matches, the device delivers the `then` as a notification **at the point of
performance** — in the moment and place the behaviour is needed. This is Barkley's clinical
prescription for externalising executive function; note that it is a clinician's framework rather
than an entry in `EVIDENCE.md`, and it is the only named authority in this document without one.
The empirical support that *is* in the evidence base is the prospective-memory work
(`EVIDENCE.md` §2.3), which found people needed prompting to check an external aid at all. It then emits a `fire` event so the log
records whether the plan actually triggered, which makes the intents themselves measurable
over time.

### 2.5 Enrichment

Asynchronous, never in the capture path, never requiring input.

**Why having the machine do this is a net gain — argued, not assumed.** Generative processing
(retrieving, organising, elaborating) is what produces durable memory; learners retain vocabulary
roughly twice as well writing definitions as copying them (`EVIDENCE.md` §2.2). Enrichment does
the organising work *for* you, so on that literature it removes exactly the effort that produces
retention. Automating this may make the archive better and your memory worse.

The counter is real and is in the same literature: **desirable difficulty effects fail when
working memory is already stressed** by high element interactivity — under those conditions added
difficulty is undesirable, not desirable. A user with ADHD, depression and anxiety, whose deficits
are additive (`EVIDENCE.md` §3), is close to a worked example of already-stressed working memory.

So the justification is **load reduction under existing stress**, not neutral convenience — and
that distinction matters, because it says where the automation should stop. It is justified for
work the user would not otherwise do at all (titling, entity extraction, transcription). It is
*not* justified as a general principle, and it is why §0.2's scheduled retrieval practice matters
more than it looks: it is the one component that deliberately puts generative effort back in.

Enrichment failure is not an error condition. An un-enriched capture is still a capture.

#### 2.5.0 The specificity pass — scoped down, and why

An earlier draft had enrichment detect categoric captures ("the meeting went badly") and had the
scheduled review later ask for the specific instance, claiming this "mirrors what memory
specificity training does clinically."

**That claim is withdrawn and the feature is constrained**, because as specified it was closer to
rumination induction than to the intervention it cited. The reasoning matters more than the
conclusion:

- c-MeST's active ingredient is **repeated practice of the retrieval operation itself**, dosed,
  with balanced cue valence, inside a supervised protocol. Annotating a note's specificity is not
  that mechanism.
- Applied to spontaneously captured material, the notes phrased categorically skew **negative** —
  the example above is the design's own. So the unconstrained behaviour was: at a
  system-chosen moment, on a system-chosen device, unprompted and unsupervised, ask a user with
  depression to generate specific detail about a bad experience. The thing c-MeST controls most
  tightly is exactly what that removes.

What remains, with guardrails that are part of the spec rather than polish:

- **Valence gate.** Prompts are only generated for neutral or positive captures. Negative
  categoric material is annotated but never prompted on.
- **Dosed.** At most one specificity prompt per review session, and none at all unless review is
  already happening.
- **Declinable as a class.** "Stop asking me this" turns the whole category off permanently, in
  one gesture, from the prompt itself. Not buried in settings.
- **Off by default**, enabled deliberately.
- **Framed as what it is** — a note-taking prompt, not a treatment. The clinical protocol is a
  clinical protocol; if the OGM finding is worth acting on, that belongs with a clinician, and
  `EVIDENCE.md` §1.4 says so.

#### 2.5.1 Enrichment is a fan-out, not a job

Treating enrichment as one pipeline forces every capture to wait for its slowest stage. It is
better modelled as independent job classes with wildly different costs, each assigned
separately:

| Class | Cost | Needs |
|---|---|---|
| `title` | milliseconds | nothing |
| `entities` | light | small model or gazetteer |
| `parse` | light | the recognizer (§3.1) |
| `links` | moderate | embedding model + vector index |
| `specificity` | moderate | classifier or LLM |
| `transcribe` | heavy, minutes | speech model, RAM, ideally GPU |
| `summary` | heavy | LLM |

Each class is assigned independently, so a phone can title a voice note within seconds while a
desktop transcribes it overnight. Nothing waits on the slowest thing.

#### 2.5.2 Capability — static in the log, dynamic out of band

Any device may enrich whatever it can actually run. Capability is not a boolean and not raw
hardware specs; it is **the set of job classes this device is willing and able to execute**,
derived at startup from a self-probe (cores, RAM, accelerator, installed models) and from
settings.

The split matters:

- **Static capability** — `device_caps`, an event in the log, signed by the device about
  itself. Changes rarely: a model is installed, hardware is replaced, a setting is toggled.
  Every device therefore agrees on who *could* run what, which is what makes assignment
  computable without any negotiation.
- **Dynamic state** — on battery, thermally throttled, already busy, on a metered network.
  Advertised in the session and mDNS TXT, **never written to the log**. This is transient
  operational chatter and the log is meant to be worth reading in ten years.

Dynamic state does not change the ranking; it changes the delay (§2.5.3). A laptop on battery
yields to an AC-powered desktop by waiting longer, not by being excluded — so if the desktop
never appears, the laptop still does the work.

Capability must be honest: declaring `transcribe` requires the model to actually be present, not
merely for the hardware to be adequate. A device that cannot deliver must not rank. Capability is
also **retractable** — a device that deletes its model emits a new `device_caps` withdrawing the
claim, otherwise it keeps winning rank 0 forever and every job pays the full delay ladder waiting
for work that will never happen.

#### 2.5.3 Assignment — rendezvous hashing, zero coordination

For each `(capture, job_class)`, every capable device computes

```
score(device) = blake3(device_id ‖ capture_id ‖ job_class)
```

sorts the devices capable of that class by score, and finds its own rank. **No messages are
exchanged and no coordination events are written.** Assignment is a pure function of data
every replica already has, which also means it lives in `capture-core` and is exhaustively
testable with no runtime, network, or clock.

Rendezvous rather than modulo because retiring a device reassigns only its share instead of
reshuffling everything.

Absence is handled by a delay ladder rather than by protocol:

```
rank 0 → start when its trigger fires
rank 1 → wait T, re-check whether a result exists; if not, run it
rank 2 → wait 2T
```

T and the trigger scale with class, because "overnight is fine" for a transcript and absurd for
a title:

| Class | Trigger for rank 0 | T |
|---|---|---|
| `title`, `parse`, `entities` | immediately | 60 s |
| `links`, `specificity` | when not actively busy | 10 min |
| `transcribe`, `summary` | on AC power and idle, or after a 24 h deadline regardless | 4 h |

Heavy work therefore lands overnight on a plugged-in machine in the normal case, and still
happens within a day if that machine never becomes idle. A phone enriches only while charging,
on unmetered network, screen off — which maps directly onto Android `WorkManager` constraints
(`requiresCharging`, `requiresDeviceIdle`, `UNMETERED`) rather than fighting the platform.

**Failure is self-healing.** A device that claims a job and then dies, sleeps, or is thermally
throttled produces no result, so the next rank simply runs after its delay. No claims, no
leases, no expiry logic, nothing to strand.

**Duplicates are possible and cheap.** During a partition, rank 0 on one side and rank 1 on the
other may both run. Merge is deterministic: for the same `(capture, job_class)`, **keep the
result from the lower rendezvous rank**.

**Ranking is frozen at the capture's HLC, not evaluated against the current roster.** This
matters more than it looks. Rendezvous ranks over "devices capable of that class", which is
derived from `device_caps` and the roster — both of which change over time. Rank against the
*current* set and retiring a device silently changes which historical transcript wins, so
re-folding the same log on Monday and Tuesday yields different results, breaking the idempotent
re-fold that §7 mode 12 depends on for recovery and that §8.2 property-tests. So: the capable set
is computed from the log prefix at the capture's HLC, and the winning rank is recorded in the
`enrich` body, freezing the choice at authorship.

That leaves one honest gap: two replicas that disagree about `device_caps` at decision time can
briefly prefer different results, and there is no divergence flag because this is designed to be
invisible. It resolves when caps propagate, but the window is as long as a device is asleep. The
frozen-rank rule keeps it transient rather than permanent, which is the part that mattered.

**No capable device is a visible state, not a silent one.** If nothing in the roster can
transcribe, the UI says so — "3 recordings awaiting transcription, no capable device" — rather
than leaving the work quietly undone forever.

### 2.6 Mute, and a notification budget

Two additions that exist because the design as originally written had no brake anywhere in it.

#### Mute — suppression without destruction

`mute { tgt }` removes a note from **review, resurfacing, and all pushed surfaces**, permanently
or until a stated date. It remains searchable and readable if you go looking for it. It is
reversible from any device, at any time.

This is the answer to "I do not want to see this again", which is a completely different need
from "this must not exist". Without it, the only exit from a painful note being resurfaced was
`redact` — irreversible, propagated destructively to every replica, and reachable while
symptomatic. A system that pushes personal content on a schedule the user is explicitly barred
from setting must have a suppress that is not destruction.

Muting is also available at coarser grain — mute a tag, a project, a date range, or a person —
because the material you do not want resurfaced usually clusters.

**And review pacing is user-overridable after all.** §0.2's "the system schedules; the user never
picks" is derived from finding 2, which is about *self-directed pacing degrading learning
outcomes* — not about denying someone a pause button. Those are not the same claim, and the
original conflated them. The system still schedules by default. But **"pause review" and "less
often" always work**, and neither requires a reason. The evidence about pacing does not survive
being turned into a rule that a person cannot stop something being pushed at them.

#### Notification budget

Anxiety is named in §0 as a core constraint and then, in the original draft, never mentioned
again — while §2.4.1 deliberately *increased* interruption frequency ("a duplicate nudge is much
better than a missed one"), justified purely on ADHD grounds with the anxiety cost never weighed.

The tradeoff still comes out the same way for genuine misses, but it needs a bound:

- **Quiet hours**, on by default, respected by everything except explicitly-armed time cues.
- **Batched delivery by default**, not merely a cap. `EVIDENCE.md` §2.5 reports a 14-day field
  experiment in which batching notifications into three daily deliveries made people happier and
  less stressed, mediated by improved subjective attention quality — as the *default regime*, not
  as an overflow behaviour. Time-critical cues (an alarm, a meeting starting) are delivered
  immediately; everything else joins the next batch.
- **Deduplication across devices within a short window**, so the accepted duplicate-nudge failure
  costs one extra buzz rather than an unbounded number.
- **A rate limiter on any single intent**, so a misconfigured cue cannot fire repeatedly.

**The "duplicate beats missed" priority stands, but the cost is now on the ledger.** §2.4.1 made
that ruling on ADHD grounds with the interruption cost unweighed twice over. `EVIDENCE.md` §2.5
supplies what was missing: a notification degrades performance on attention-demanding tasks
**even when it is not acted on**. So a duplicate is not free, and the ruling is a trade rather
than an obvious call — it survives because a missed intent fails silently at the one thing the
intent existed to do, and because batching plus cross-device dedup keeps duplicates from
compounding into a stream.

### 2.7 Review does not grade you

An earlier draft defined the `review` event's outcome as **recalled / missed / snoozed**. That is
a graded memory test, administered on a schedule the user does not set, to a person whose stated
difficulty is memory, with the result **written permanently to an immutable log** and fed back
into future scheduling.

`EVIDENCE.md` §3 reports that anxiety in ADHD is frequently secondary to *sustained executive
strain and repeated performance failures*. If that mechanism is right, a recurring,
indefinitely-repeating, permanently-recorded record of memory failures is the single most harmful
thing this design could contain — and it is the one surface that never got examined, while §3.6
spent twenty lines interrogating whether a completion percentage is gentler than a streak.

So:

- **The outcome is `shown / engaged / deferred`.** Whether the item appeared, whether you did
  anything with it, whether you pushed it away. No judgement about your memory is recorded,
  because the system does not need one to schedule well — engagement is a sufficient signal.
- **There is no score, streak, accuracy, or history of failure**, anywhere, at any aggregation.
- **`mute` applies to review** (§2.6). Anything you do not want resurfaced stops being resurfaced,
  reversibly, without destroying it.

Scheduled retrieval practice is the component with the cleanest evidence behind it (§0.1 finding
2), and it stays. What goes is the part the evidence never asked for: keeping score.

### 2.8 Review — the component with the best evidence, finally specified

Until now this was a table row, an event kind, and a deferred open question. That was backwards:
scheduled retrieval practice under **imposed** pacing is the one component in this design with a
clean mapping to a finding (§0.1 finding 2 — retrieval practice benefits ADHD equally; it is
*self-directed* pacing where the ground is lost). It is also the only thing here that pushes back
against the offloading objection in §0.3.1, because it is the one part that deliberately puts
generative effort back in.

**Session shape.** Bounded and ending: a fixed small number of items, then it is over and says so.
An unbounded queue is a backlog, and §3.6 spends its length explaining why manufactured backlogs
are the wrong shape for this user.

**Selection**, in priority order:

1. Notes carrying a forward marker the recognizer found — "revisit Q2", "check back in a month"
2. Notes attached to an active project or an upcoming calendar event
3. Notes never engaged with since capture, oldest first
4. Notes whose ambient context matches your current context (§2.3) — same repo, same person

**Scheduling.** Expanding intervals, but tuned by **engagement, not accuracy**. Standard spaced
repetition schedules on whether you got an item right; there is no right here, and §2.7 removes
the graded outcome that such a schedule would need. An item you engage with recurs less often
because it has landed; an item you defer twice stops being offered rather than being pressed.

**Nothing is scored.** No streak, no accuracy, no aggregate. See §2.7 — this is the surface where
a graded record would do the most harm, and the scheduling works without one.

**Muting always wins** (§2.6), and "pause review" and "less often" always work without a reason.

**Honest limit:** the spacing literature (`EVIDENCE.md` §2.4, §1.1) is about factual material in
educational settings — d = 0.54 for distributed practice in classrooms, and effects that vary by
domain. Nobody has shown that resurfacing personal episodic notes on an expanding schedule helps
anything. This is the best-evidenced component *here*, which is a statement about the rest of the
design as much as about this.

### 2.9 What this does not solve — stated plainly

This design reduces capture cost to roughly one second and one gesture. It does **not** remove
you from the loop. During a live call, one second is still one second, and you may still not
take it.

That is the honest limit, and it is the same limit the article you read identified. The
alternative — always-on ambient recording of calls and screen — would close that gap, and it is
deliberately not in this design. It records other people who have not consented, it is
recording in jurisdictions where that may not be legal, and it creates a permanent searchable
archive of everything you have ever said near a microphone. If you want that capability, the
right shape is a **bounded, explicitly-armed session** — you arm it for one specific call, it
auto-disarms — and it should be designed as a separate module with its own retention policy,
not folded into always-on capture. Worth discussing before building.

---

## 3. Time, tasks, and organisation

Adding calendars, recurring events, todo lists and organisation is where this design could
quietly become an ordinary productivity app — the kind §0 says fails. Two rules keep it from
happening, and everything in this section follows from them.

### 3.1 Types are promotions, not choices

**There is still exactly one capture verb.** You never pick a type. Nothing in the capture path
asks "is this a note, a task, or an event?" — that question is a working-memory allocation at
the worst possible moment, and answering it wrongly is why things end up unfindable.

Instead, every input lands as a `capture`, and **type is derived**:

```
"dentist thursday 2pm"            → timed event, Thu 14:00, 30m default
"walk the dog every day at 7am"   → recurring obligation, daily 07:00
"walk the dog every day at 7"     → recurring obligation, daily, untimed — "7am or 7pm?"
"vacation aug 1-14"               → all-day event, floating dates
"send sarah the migration doc"    → task, no due date
"budget 40k, revisit in Q2"       → note, with a Q2 review nudge
```

Parsing happens in enrichment (§2.5), asynchronously, and emits a `promote` event. Three
safety properties, all non-negotiable:

1. **A failed or wrong parse never loses the input.** If parsing fails entirely, it stays a
   note — still captured, still searchable, still surfaced. The parse is an enhancement, never
   a gate.
2. **Promotion is reversible.** `promote` is an ordinary event; a later one supersedes it. Fix
   a misparse from any device, whenever you notice.
3. **Capture does not wait for it.** The overlay closes on `Enter`. The 150 ms budget is
   untouched because parsing happens after the commit, not before.

The capture overlay may *show* what it thinks it heard as passive text once parsing is fast
enough to be local — but `Enter` commits regardless, and there is nothing to confirm, accept,
or dismiss. A prompt would reintroduce the decision.

### 3.1.1 This is a recognizer, not a DSL

Worth stating flatly, because the difference decides whether the system is usable:

| | DSL | Recognizer (this design) |
|---|---|---|
| Input | Must conform to a syntax | Anything you type |
| Parser role | Authoritative | Advisory |
| On failure | Syntax error | It stays a note |
| Cost to you | Learn and recall the syntax | Zero |

**A DSL would be the wrong answer here** for exactly the reason the whole design exists. It
moves a cost to the worst possible moment: you would be composing syntax mid-call instead of
capturing a thought, and a malformed line would either be rejected or silently mean something
you did not intend. A syntax you must recall under load is a working-memory tax, and working
memory is the resource already under pressure from all three directions in §0.

So: **the input language is English, and there is nothing to get wrong.**

**Optional sigils, never required.** For the times you already know exactly what you want, a
small set of explicit markers is recognised and always beats inference:

```
#tag   @person   >2026-08-24   !p1   ~30m   ^project
```

They are an accelerant, not an interface. Nothing is unreachable without them, nothing breaks
if you never learn them, and they are discovered by seeing them in the review view rather than
taught by a manual. When a sigil and inferred text disagree — `>fri` in a line that also says
"thursday" — the explicit sigil wins, because you typed it deliberately.

### 3.1.2 Three tiers, in order

1. **Sigils** — exact, trivial, always available offline.
2. **Grammar** — a deterministic parser for the common shapes (`thursday 2pm`, `every weekday
   at 7`, `aug 1-14`, `in 3 weeks`). Fast, offline, unit-testable, and covers the large
   majority of real input.
3. **Model fallback** — an LLM pass for anything the grammar rejects. Handles messy phrasing,
   but needs a capable device awake.

The grammar wins wherever it applies. Determinism matters more than coverage here: a rule you
can test and predict is worth more than a model that is right slightly more often but
differently each time. The model only sees what the grammar declined.

### 3.1.3 Promotion is additive, never subtractive

**The captured text is preserved verbatim, forever.** Promotion annotates it; it never rewrites,
consumes, or relocates it. This is the property that makes false positives survivable.

False positives are the real hazard in natural-language recognition, and they are worse than
false negatives. "the march timeline slipped" contains a month. "the 7am problem" contains a
time. A parser that consumed those into a calendar entry, and moved the text out of your note
stream to do it, would be actively destroying information — the exact failure this system
exists to prevent. Because promotion only adds a structured overlay, a false positive is a
wrong label on an intact note, not a lost note.

Two further guards:

- **Confidence floor.** Below the threshold, nothing is promoted. The parse is recorded as a
  *suggestion* and surfaces in review as one line. The system never silently guesses.
- **Human corrections pin.** A `promote` authored by you carries `pinned: true`, and no
  reparse ever overrides it (§6). This matters because the parser will improve, and reparsing
  the backlog is desirable — but silently reverting a correction you already made would teach
  you the system cannot be trusted, which is unrecoverable.

### 3.1.4 Text versus structure — which is the truth

Once a note is promoted, the two can drift: the text says "every day at 7", but you later
changed the series to weekdays in a form. Systems that try to keep text and structure in
two-way sync end up in a swamp, so this design does not.

Rule: **the captured text is the episodic record; the structured fields are operationally
authoritative; neither is regenerated from the other.** A note carries three things —

1. `text` — what you typed or said. Verbatim, never rewritten.
2. `fields` — current structured state. What actually fires and appears on the calendar.
3. `descriptor` — a one-line human rendering of `fields`, regenerated whenever they change.

You always see both what you captured and what it currently is, so drift is visible rather than
silent. Editing the text triggers a reparse (respecting pins). Editing via a form updates
`fields` only. No round-trip, no ambiguity about which one wins.

**Deliberate creation is a separate door.** Sometimes you genuinely sit down to schedule a
meeting for next Tuesday. Doing that from the calendar view, with a real form, is fine — it is
a considered act, not capture. Two entry points: one that is instant and typeless, one that is
deliberate and structured. The mistake other tools make is having only the second.

### 3.1.5 The vocabulary, and the grammar over it

§3.1.2 fixes the tiering and leaves the middle tier undescribed, which is the tier that decides
whether any of this works: sigils cover what you already know you want, the model covers
whatever is left, and everything in between is tier 2. "A deterministic parser for the common
shapes" is a commitment to *have* a vocabulary, not a vocabulary. This section is that list and
the grammar over it.

**The vocabulary is closed, and that is the point.** Every word tier 2 recognises is enumerated
below. A closed list is what makes the tier testable — the test suite can be exhaustive over the
language rather than representative of it — and it is what makes the failure mode legible: a word
that is not in the list is not a near-miss, it is tier 3's problem, and tier 3 declining leaves an
ordinary note (§3.1.3). Growing the list is a deliberate edit with tests attached, never an
inference.

#### The closed classes

| Class | Members |
|---|---|
| `weekday` | monday, tuesday, wednesday, thursday, friday, saturday, sunday + `mon tue tues wed thu thur thurs fri sat sun` |
| `month` | january … december + `jan feb mar apr jun jul aug sep sept oct nov dec` |
| `day_word` | today, tonight, tomorrow, yesterday |
| `tod_word` | midnight (00:00), morning (09:00), noon (12:00), afternoon (14:00), evening (18:00), night (20:00) |
| `unit` | minute, hour, day, week, month, year, quarter + `min mins hr hrs wk wks mo mos yr yrs` |
| `freq_word` | daily, nightly, weekly, monthly, quarterly, yearly, annually, hourly |
| `rec_head` | every, each |
| `day_set` | weekday, weekdays, weekend, weekends |
| `determiner` | next, this, last, coming, upcoming |
| `marker` | in, at, on, by, due, from, before, after, starting, until |
| `range_op` | `-`, `..`, to, through, thru, until |
| `meridiem` | am, pm, a.m., p.m. |
| `interval` | other (as in "every other week") |

Two classes carry a fixed numeric meaning rather than a lookup: `quarter` is `Q1`–`Q4`, and
`ordinal` is `1st`–`31st`.

**`at` is not special.** It is one marker among ten, and it means nothing on its own — "meet me
at the pub" contains it and no time. Markers exist to give a following expression permission to be
temporal, never to be temporal themselves. The same is true of every word above: none of them is
a keyword you have to reach for, because §3.1.1 already committed to a recognizer over English
rather than a syntax you compose. The list is what the recognizer *reads*, not what you have to
*write*.

#### The grammar

```ebnf
capture     = { sigil | temporal | word } ;

(* tier 1 — always wins over anything inferred, §3.1.1 *)
sigil       = "#" slug | "@" slug | ">" ( iso_date | day_ref )
            | "!" "p" digit | "~" number unit | "^" slug ;

(* tier 2 *)
temporal    = recurrence | range | point ;

recurrence  = rec_head [ interval | number ] rec_body [ [ "at" ] clock ]
            | freq_word [ [ "at" ] clock ] ;
rec_body    = weekday | day_set | unit ;

range       = month day_num range_op day_num [ year ]
            | point range_op point ;

point       = [ marker ] anchor [ [ "at" ] clock ] ;
anchor      = iso_date
            | month day_num [ year ]
            | day_num month [ year ]
            | weekday
            | day_word
            | quarter
            | "in" number unit
            | determiner ( weekday | unit | quarter ) ;

clock       = hour [ ":" minute ] [ meridiem ]
            | tod_word ;
```

Everything the five worked examples in §3.1 need, and nothing else. `dentist thursday 2pm` is
`point( anchor=weekday, clock=hour+meridiem )`; `walk the dog every day at 7` is
`recurrence( rec_head, rec_body=unit, clock )`; `vacation aug 1-14` is the first `range`
production; `in 3 weeks` is the `"in" number unit` anchor.

A bare hour after a marker is a clock only when it says which half of the day: `0` and
`13`–`23`. A bare `1`–`12` is not guessed (changed 2026-10-12, grammar 7): `at 7` promotes the
day or the rule without a time and asks "7am or 7pm?", and the answer is an ordinary edit that
inserts the meridiem. The guess this replaced read `at 7` as 07:00, which is right for a dog
walk and put "gym every monday at 2" at 2 am.

A day is treated the same way (grammar 9, 2026-10-13). A weekday beside a complete date —
`thursday oct 22`, `Thursday 2026-10-22`, `oct 22, thursday` — is that date when the two agree,
and asked when they do not ("Thu 22 Oct or Fri 23 Oct?"). `next thursday` is asked while this
week's Thursday is still ahead, since it is this week's to some people and next week's to others;
once it has passed, both readings are the coming one, and said on a Thursday it is a week away.
`last thursday` is the most recent before today. An unclear day promotes nothing and is shown on
both candidate days of the calendar; the answer rewrites the day in the text. And a correction —
an accepted suggestion, a kind set by hand — can be taken back ("read it again"), because a pin
outlives every edit and an accepted misreading otherwise could never move again.

#### The guards, which matter more than the grammar

A context-free grammar over this vocabulary would fire constantly on ordinary prose, and §3.1.3
is explicit that false positives are the worse error. So recognition is gated by rules that are
deliberately not part of the grammar above, because they are about what *surrounds* a match:

1. **An article kills it.** A `month`, `weekday` or bare `clock` immediately preceded by `the`,
   `a`, `an`, or a possessive is not temporal. This is what makes "the march timeline slipped"
   and "the 7am problem" — both §3.1.3's own examples — parse as prose.
2. **A bare number is never a time.** `clock` requires a meridiem, a colon, a `tod_word`, or a
   marker in front of it. "budget 40k" and "40 users" cannot become 40:00.
3. **A following noun kills a bare weekday or month.** "march timeline", "friday deploy" — the
   temporal word is modifying something, so it is a name, not a date.
4. **Phrase boundaries bound a match.** A temporal expression must start at the beginning of the
   capture, at a comma, or after a marker. It cannot start mid-clause.
5. **Sigils pre-empt.** Where a sigil and an inferred expression disagree, the sigil wins and the
   inference is not recorded at all — not even as a suggestion (§3.1.1).

#### Confidence, and what crosses the floor

§3.1.3 requires a confidence floor and never says what it is. **It is 0.75**, and the bands are
fixed rather than tuned per-shape so a misfire is diagnosable:

| Confidence | Shape |
|---|---|
| 1.00 | a sigil — not scored, and not overridable by inference |
| 0.90 | a complete date or datetime behind a marker: `on aug 14`, `at 2pm`, `by friday 9am` |
| 0.85 | two vocabulary items adjacent and unambiguous: `thursday 2pm`, `aug 1-14`, `in 3 weeks` |
| 0.80 | a recurrence head with a body: `every weekday at 7`, `daily` |
| 0.80 | a task verb and a bare day: `call the dentist tomorrow`, `email sarah on friday` |
| 0.70 | a bare weekday or day_word with no clock: `dentist thursday` |
| 0.60 | a `tod_word` standing in for a clock: `tomorrow morning` |
| 0.40 | a task verb with no temporal expression at all: `send sarah the migration doc` |

Below 0.75 nothing is promoted; the parse becomes a one-line suggestion in review, which is what
§3.1.3 already specifies. That places bare weekdays, vague times of day and every task-verb guess
under the floor deliberately — those are exactly the readings most likely to be wrong, and a wrong
suggestion costs a glance while a wrong promotion costs a calendar entry.

**The task verbs are a suggestion tier, not a class.** `send call email buy ask book fix write
review pay order check ping schedule renew cancel submit file print` at the head of a capture,
with no temporal expression anywhere, scores 0.40. Verbs are an open class and any list is wrong
at the edges, so this one never promotes on its own — it exists so review can offer "task?" on a
line that plainly is one, and no further.

**A verb and a day compose, and that is the one place the tiers meet.** Written after a parser
existed and the first draft of this section did not survive contact with it: a task verb in front
of a bare day was read as the day alone, and the verb was thrown away. "Call the dentist
tomorrow" became an all-day event on Thursday. That is wrong by §3.6's own table — an event
occupies time and has no state, a task has state and occupies none, and calling the dentist does
not occupy Thursday.

So a task verb at the head plus a **bare day** is a task with a floating due date, at 0.80. Two
conditions, both load-bearing:

- **Only a bare day composes.** A clock means something that occupies time whatever verb
  introduced it: `call sarah at 3pm` is a call you are on at three, and stays an event. A range
  or a recurrence is already a shape of its own.
- **The day stays floating.** "Due Friday" is not 00:00 UTC on Friday (§3.6), and storing an
  instant would put a midnight on the calendar nobody wrote.

0.80 is above the floor, which is a decision rather than a nudge, and the argument is the cost
asymmetry this section already rests on. §3.1.3 prices a false positive as a wrongly created
*calendar entry* — something with a time, a position on a grid, and a notifier that can fire it.
A wrongly created task has none of those: it is a line in a list with a date on it, removed by an
ordinary correction. It is kept below 0.85 all the same, because two adjacent vocabulary items
are a stronger claim about what was *written* than a verb plus a day, which is a claim about what
was *meant*.

#### What is deliberately absent

No place names, no person names, no project names, no priority words, no durations in prose ("a
couple of hours"), no fuzzy quantities ("a few weeks"). Every one of them is either a sigil
already — `@person`, `^project`, `!p1`, `~30m` — or open-class, which means it belongs to tier 3
or to nothing. The vocabulary stops where determinism stops.

### 3.2 Time model — instants versus floating dates

Most calendar bugs come from storing one kind of time where the other belongs. There are two
kinds and they are not interchangeable.

**Timed events** (a 2pm meeting) are **instants**. Stored as UTC milliseconds *plus* the IANA
timezone id they were created in. Both are required: UTC alone cannot expand a recurrence
correctly, because "07:00 daily" means 07:00 local on both sides of a DST boundary, which is
not a fixed UTC offset.

**All-day events** (a vacation, a birthday, a deadline) are **floating dates**. Stored as plain
`YYYY-MM-DD`, with no timezone and no conversion, ever. A vacation from 1–14 August is 1–14
August in Tokyo and in Denver. Converting all-day events to UTC instants is the single most
common calendar bug in the wild — it makes events silently jump a day for users east or west of
the storing machine.

```jsonc
// timed
{ "kind":"promote", "as":"event",
  "start": 1755781200000, "end": 1755784800000, "tz": "America/Denver" }

// all-day — no instants anywhere
{ "kind":"promote", "as":"event",
  "start_date": "2026-08-01", "end_date": "2026-08-14", "all_day": true }
```

**DST policy, stated so it is not decided by accident:** recurrences expand in the series'
local wall time, then convert to instants. For a nonexistent local time (spring forward, e.g.
02:30 on a day that jumps 02:00→03:00) the occurrence shifts forward to the first valid
instant. For an ambiguous local time (fall back, 01:30 occurring twice) the first occurrence
wins. Both are the RFC 5545 conventions, and both are recorded on the expanded occurrence so
the view can show that a shift happened rather than silently lying.

Devices carry their own tz database and it will occasionally disagree between an Android phone
and a desktop that has not updated. Expansion therefore happens **in the projection, per
device**, and is rebuildable — so a tz database update is fixed by re-folding, not by a
migration.

### 3.3 Scheduled events

A one-off event is a `promote` to `as: "event"` on a capture, carrying start, end, all-day
flag, timezone, and optionally location and attendees. It remains the same note underneath —
the text, the ambient envelope (§2.3), tags and links all still apply. **An event is a note
with a time**, not a separate species of object. This matters for browsing: the meeting and the
notes you took during it are the same record, not two things you have to associate later.

### 3.4 Recurrence

The rule is stored; **occurrences are never written to the log**.

Storing an RRULE (RFC 5545 subset: `FREQ`, `INTERVAL`, `BYDAY`, `BYMONTHDAY`, `COUNT`, `UNTIL`)
keeps the log finite for an infinite series, and keeps a daily habit from generating 365 events
a year in a log that syncs to a phone. Occurrences are expanded lazily into `state.db` over a
rolling horizon (−1 year, +1 year), extended in the background, and fully rebuildable — they
are projection data, not truth.

Truth is: the rule, plus a set of exceptions.

### 3.5 Occurrence identity and exceptions

An occurrence is named by **`(series_id, original_start)`** — the series root plus the UTC
instant the rule *would* have produced, before any move. This is the iCalendar `RECURRENCE-ID`
approach and it is the right one, because it gives a stable name to an occurrence **that has
never been materialised anywhere**. You can complete, skip, or move next Tuesday's gym session
without anything named "next Tuesday's gym session" existing in any table.

Exception events all carry `tgt = series_id` and `occ = original_start`:

| Kind | Meaning |
|---|---|
| `occ_move` | This occurrence only: new start/end. |
| `occ_edit` | This occurrence only: different title, location, notes. |
| `occ_cancel` | This occurrence only: skipped. Series continues. |
| `occ_done` | Completion (recurring obligations, §3.6). |

**Edit scope — the part implementations get wrong.** When you change a recurring event, there
are three possible intents and they need genuinely different mechanics:

| Scope | Mechanism |
|---|---|
| **This occurrence** | An `occ_*` exception event. Rule untouched. |
| **All occurrences** | An ordinary `edit` on the series rule. Past occurrences change too. |
| **This and future** | **Split the series.** Bound the existing rule with `UNTIL = occ − 1`, create a new series starting at `occ` with the new rule, linked by `split_from`. |

The split is what keeps history honest. Editing the rule in place would silently rewrite what
your gym schedule *was* last March, which turns the log into an unreliable narrator — fatal for
a system whose whole purpose is compensating for unreliable memory. Splitting leaves the past
exactly as it happened.

Exceptions survive a split by binding to whichever series covers their `original_start`.

### 3.6 Tasks, and why recurring tasks do not stack

Three things get conflated everywhere and are kept distinct here:

| | Occupies time | Has state | Missing it means |
|---|---|---|---|
| **Event** | yes | no | it passed |
| **Task** | no | open / done | still open, carries forward |
| **Recurring obligation** | optionally | per-instance | that instance lapsed — *nothing accumulates* |

A **task** is a `promote` to `as: "task"`: state, optional due date (floating date, not an
instant — "due Friday" is not 00:00 UTC Friday), optional list membership, optional ordering.
Lists are just tags with a display order, so a task can sit in several without duplication.

A **recurring obligation** ("walk the dog daily", "get up at 7") is a recurring series whose
occurrences carry completion state. Each instance completes independently via `occ_done`.

**By default, uncompleted instances do not roll over, do not stack, and do not accumulate.** Miss
Tuesday's walk and there is no debt on Wednesday — Tuesday's instance lapses and is recorded as
lapsed. This is a deliberate departure from how nearly every todo app behaves, and it comes from
§0:

- The habit literature is explicit that a single missed day costs almost nothing — under half a
  point of automaticity, recovered quickly. A system that manufactures a backlog is therefore
  representing something that is *not true* about how habits actually form.
- A visible pile of overdue recurring items is an all-or-nothing failure signal. **An earlier
  draft called this "precisely the categoric, overgeneral pattern the depression research
  identifies as the thing to avoid reinforcing" — that framing is withdrawn**, and it is the same
  conflation §2.3 already withdrew. OGM is about the specificity of autobiographical *retrieval*;
  it is not the CBT construct of all-or-nothing thinking, and nothing in `EVIDENCE.md` studies
  backlog displays. The correction was applied where it was noticed and not propagated here.

  What the evidence does support is narrower and, as it happens, stronger: `EVIDENCE.md` §3
  reports anxiety in ADHD as frequently secondary to **repeated performance failures**. A
  manufactured pile of failures is a mechanism-level concern on that account, not a metaphor.

#### `carries_forward` — the exception that has to exist

Applying "nothing accumulates" to the whole class is a foot-gun, and a serious one. The
recognizer (§3.1) assigns "recurring obligation" to anything phrased recurrently, which includes
obligations that genuinely **do** accumulate:

> a missed medication dose. An unpaid invoice. A prescription refill. A weekly report.

For those, missing Tuesday means Wednesday carries a real debt, and silently dropping it is worse
than the ordinary todo-app failure this rule was written to fix — especially for a user whose
presenting difficulty is forgetting things. "Recorded as lapsed" is not the same as "still owed".

So: a per-series **`carries_forward`** flag, defaulting to false. When set, uncompleted instances
accumulate and remain visible until completed or explicitly dismissed. The recognizer never sets
it automatically — inferring "this one really matters" from phrasing is exactly the kind of guess
that must not be made silently — but a series that has lapsed repeatedly prompts once, in review,
asking whether it should carry forward.

The non-accumulating default is right for habits. It was never right for obligations, and the
original draft had no way to say which was which.

#### No streak counters — a judgement call, labelled as one

A streak makes the 30th day catastrophic to lose, which inverts the dynamics the habit literature
describes: a single missed day costs almost nothing and recovers quickly. The system tracks a
**rolling completion rate over a trailing window** (14 and 60 days) instead.

**This is a plausibility argument, not a finding.** Lally modelled automaticity against
repetitions and elapsed days; it did not test consecutive runs against aggregate rate as
competing predictors, and nothing in `EVIDENCE.md` tested streak counters or backlog displays at
all. An earlier draft said "the question the research says predicts automaticity", which
overstated what was measured.

And the replacement may not be strictly better, which is worth writing down rather than
discovering later. A broken streak resets to zero and can be rebuilt **starting tomorrow**. A
60-day trailing rate is a slow-moving number that stays bad for two months no matter what you do
today — arguably a closer match to a hopelessness signal than the thing it replaced. Both
displays quantify failure; only one of them can be improved by acting immediately.

**So completion metrics are off by default.** `EVIDENCE.md` §3 moves this from a coin-flip
between two guesses to something with a direction: if anxiety here is secondary to repeated
performance failure, then a display whose function is to quantify your failure rate is a cost
whichever shape it takes, and neither candidate is defensible as an on-by-default. Turn them on if
you find them useful; nothing depends on them being visible.

When shown: the 14-day window by default and the 60-day only on request, never a bare percentage
without the trend beside it, and any series can hide its metrics entirely. **§9 keeps this open** —
it is a design guess about motivational dynamics, and it should be judged by whether it feels
punishing in practice rather than by citation.

The lapse data is still *recorded* either way, because the system needs it to notice that a 07:00
cue has failed 11 of 14 days and suggest the cue is wrong. Recording it and displaying it are
different decisions.

Lapsed instances are still *recorded*, because the data is genuinely useful: it feeds the
rolling rate, and it is what lets the system notice that the 07:00 gym plan has lapsed 11 of
the last 14 days and suggest the cue is wrong rather than that you are.

### 3.7 Organisation without filing

**Collections are saved queries, not folders.** A note is never *moved into* a collection; it
either matches or it does not. This is the only structure that costs nothing at capture time,
and it has no orphans, no "where did I put it," and no wrong answers to undo.

Four organising axes, all of which populate without you:

| Axis | Filled by |
|---|---|
| **Tags** | Enrichment suggests; you correct. OR-Set, add-wins (§6). |
| **Entities** (people, projects, clients) | Extracted from text and transcripts. |
| **Ambient context** (app, repo, branch, location, calendar event) | Captured automatically (§2.3). |
| **Time** | Free — every event has an HLC. |

Explicit **pins** exist for the small number of things you genuinely want placed by hand.

If a PARA-style split is wanted, it is four saved queries — Projects, Areas, Resources,
Archive — not four folders you must file into. The distinction is the whole point: PARA's
weekly filing ritual is exactly the kind of maintenance that has an excellent chance of not
happening.

### 3.8 Activation — activity scheduling, and what it deliberately is not

**Behavioural activation has the largest effect in this evidence base and is the only one of them
that software can actually deliver**: g = 0.85 individual, g = 0.69 internet-delivered,
indistinguishable from CBT across 15 RCTs (`EVIDENCE.md` §5). Until now it was absent from this
design entirely — which is what let §9 Q0 conclude "the alternatives are all things software
cannot become", when one of them plainly is not.

Its mechanism is a loop this design is already shaped to interrupt: depression reduces activity,
reduced activity removes sources of reward, and diminished reward deepens withdrawal. BA breaks in
at the behavioural end — schedule activities **in advance and independently of motivation**, note
how they actually went, and use that to find what genuinely helps rather than what you assume
helps.

**Almost all of the machinery already exists.** An activity is a `series` or a one-off in
`events_at`. Reminders are `intent`s firing at the point of performance (§2.4). Completion is
`occ_done`. What is new is one event kind and one view:

```jsonc
{ "kind": "rating", "tgt": <activity>, "occ": <occurrence>,
  "mastery": 3, "pleasure": 1 }     // 0–4, both optional, either may be omitted
```

And the view that makes it worth collecting: **which activities precede better days.** That is the
functional-analysis half of BA, it is the half that requires data, and it is the reason the
ratings exist at all.

#### What this is not, stated as firmly as the specificity pass

BA is a **clinical protocol**, delivered with a therapist, with graded assignment and functional
analysis and a formulation. This is **activity scheduling with annotation** — the structural part
a piece of software can carry. Calling it behavioural activation would be the same error §2.5.0
withdrew for c-MeST, and it is not made here.

#### Anti-harm rules, which are part of the spec

`EVIDENCE.md` §2.9 documents real harms from apps that surface assessments of the user without
support, and §3 reports anxiety in ADHD as frequently secondary to repeated performance failure.
A self-rating feature is precisely the shape that goes wrong. So:

- **Ratings are of the activity, never of you.** "Was that worth doing" is a different question
  from "did you do enough", and only the first is ever asked. This is the same line §2.7 draws for
  review, and it is what makes the two compatible.
- **Every rating is skippable, always, and never chased.** A skipped rating is missing data, not a
  missed obligation.
- **No wellness score, no mood graph, no trend line as a verdict.** The output is always a
  statement about activities — *"days with a walk in them rate higher than days without"* — which
  is actionable. A number describing you is not.
- **Scheduled activities do not stack when missed** (§3.6), and `carries_forward` stays off for
  them. A pile of unmet activity commitments is the exact opposite of the intervention.
- **Opt-in.** Someone using this as a note-taker should never be handed a therapeutic module they
  did not ask for. Enabled deliberately; prominent once enabled.
- **It says what it is on enabling**: scaffolding derived from an evidence-based therapy, not the
  therapy, and not a substitute for a clinician.

### 3.9 External calendars — a deliberate boundary

Work meetings usually already live in Google Calendar or Exchange, and reimplementing CalDAV
is a large, thankless, permanently-broken commitment.

Boundary: **external calendars are imported read-only into a shadow namespace.** They appear in
the calendar and agenda views, and they are available as ambient context (§2.3), but they are
never authored into the log as owned events and are never written back. Your notes *about* a
meeting are yours and live in the log; the meeting itself stays a mirror of someone else's
system. If the external calendar disappears, the notes survive with their captured context
intact.

---

## 4. Views

Browsing needs several axes, not one, and the reason is in §0: when encoding is the weak link,
you frequently cannot recall the note itself but can recall the *situation* — what you were
doing, where you were, who was there. The system captured all of that (§2.3), so it should be
navigable by all of it.

### 4.1 One substrate, two layouts, one mode

An earlier draft listed seven views as though they were seven screens. They are not, and treating
them as such would mean seven places to choose between — which §0.2's best-supported rule says is
itself a cost.

| §4 name | What it actually is | Row identity | Ordered by |
|---|---|---|---|
| **Timeline** | **substrate A** — everything, one stream | note | capture time |
| Context (app, repo/branch, location, meeting) | timeline + filter | note | capture time |
| Entity (person, project, tag) | timeline + filter | note | capture time |
| Search | timeline + query (FTS5) | note | relevance |
| **Agenda** | **substrate B**, list layout | occurrence | scheduled time |
| **Calendar** | **substrate B**, grid layout | occurrence | scheduled time |
| **Review** | a different **mode** — one item at a time, pushed (§0.2) | note | system-chosen |

**Two substrates, not one.** An earlier draft claimed agenda was "timeline + filter", which is
false in two independent ways: the timeline is ordered by capture time and an agenda by scheduled
time, and — more fundamentally — **agenda rows are occurrences, not notes.** One recurring series
produces many agenda rows, and an occurrence may never have been materialised as anything (§3.5).
No filter over a capture-ordered note stream produces tomorrow's agenda.

So the honest shape is: **two substrates, three layouts, one mode.** Still small, and now the
claim is true. Filters compose within a substrate; there is nothing to learn beyond "click a thing
to narrow to it."

### 4.2 The timeline

**Grouped by capture day; within a day, ordered by capture time. Human edits bump a note to the
top of its own day; machine activity never moves anything.**

The rule needs stating that precisely, because an earlier draft said "sorted by last human edit"
while its own worked example showed rows at 14:32 / 11:05 / 09:47 — capture times, which an
edit-time sort would scramble the moment anything was touched.

"Human edit" is defined structurally rather than by trusting a flag: the event kinds `edit`,
`tag`/`untag`, `alias`, `promote`, `occ_done`, `mute` and `resolve` are human when authored
interactively, and the fold distinguishes them from enrichment because **enrichment only ever
emits `enrich`**. It never emits `edit`. So the distinction is a kind check, not a `pinned: true`
self-assertion in a body written by the same process that runs the parser.

Machine activity must not reorder the view — a transcript landing, an entity pass, a reparse sweep
over the whole backlog (§7 mode 20b) — or the timeline reshuffles on a schedule you did not set
and cannot predict. Enrichment updates a note in place.

**Day grouping uses the capture's local timezone**, recorded in `notes.tz` at capture. Without it
a note captured at 23:40 in Tokyo files under a different day on a Denver desktop — and travel is
precisely when situational cues matter most, which is the whole argument for day grouping. The
timezone must be captured at capture time; it cannot be recovered later.

**Events appear at capture time, not scheduled time.** A meeting captured Monday for Friday sits
under Monday. The timeline answers *when did I write this*; the calendar answers *when is it*.
Merging the two would make the timeline misreport your own history.

**The row.** The recognition cue is frequently the situation rather than the text — that is the
entire point of collecting an ambient envelope (§2.3) — so context is visible by default rather
than hidden behind a hover or a click. It costs a line of vertical space and earns it back the
first time you find something by remembering where you were.

```
── Tue 18 Aug ────────────────────────────────────

  14:32   budget 40k → 55k, Sarah owns, revisit Q2
          ▸ acme-migration · branch: rework-auth        ⌗client-acme

  11:05   ▣ dentist — Thu 21 Aug, 14:00
          ▸ captured during "Standup"

  09:47   ◉ voice 0:34 · transcribing…

── Mon 17 Aug ────────────────────────────────────

  16:20   the deploy keeps timing out on staging   ⚠ diverged
          ▸ Terminal · brain/deploy.sh

  09:12   [redacted 20 Aug]
```

Every element of the context strip is clickable and filters the timeline to it. That *is* context
browse — no separate screen exists or needs to.

**Four states that must render, not be treated as edge cases:**

| State | Renders as |
|---|---|
| Un-enriched | `◉ voice 0:34 · transcribing…` — never a blank row |
| Redacted | `[redacted 20 Aug]` — visible, per §1.1; a silent gap breeds doubt |
| Diverged | a marker, expandable to both versions side by side. Never a blocking prompt |
| Muted | a subtle marker. Muting suppresses *pushed* surfaces (§2.6), not browsing — you should be able to see why something stopped resurfacing |

**No "+ New note" button.** Capture is the same hotkey everywhere, including from inside the app.
An on-screen button becomes the path of least resistance, and then the gesture is no longer
uniform — undermining the one habit lever with actual evidence behind it (§0.1 finding 4).

### 4.3 Wiki navigation

Notes are browsable as linked pages. This looks like the machinery §0 is sceptical of, so the
boundary matters: what fails is **requiring organising work at capture time**. Links built after
the fact, or by enrichment, do not touch that.

The rule that keeps it honest is the same one governing sigils (§3.1.1): **navigation must work
with zero manual linking.** Manual links are an accelerant, never a requirement. Otherwise this
becomes the weekly filing ritual that has an excellent chance of not happening.

#### Names that are words

A note's identity is `tgt`, a blake3 hash — robust, and unusable in prose. So every note also
has a **canonical slug**: a few significant words from its *root capture text*, plus **ten**
base32 characters of `tgt`.

```
capture:  "budget 40k, revisit Q2"   →   [[budget-40k-revisit-q2-k7m2xd4p9r]]
```

Derived from the root `capture` event's body — which is immutable (§1) and, importantly, is *not*
the current note text. An earlier draft's own example derived a slug containing "sarah" from a
string that only existed after an edit; the rule and the example contradicted each other in the
same paragraph. The rule is the root capture, always.

**The suffix is ten base32 characters (50 bits), not four hex (16 bits).** Sixteen bits is not
collision-proof and the claim that it was is withdrawn: identical short captures are routine here
— §1 itself uses "milk" typed twice as an example — and at a few hundred repeats of any common
phrase a collision is likely rather than exotic. Because `notes.slug` is `UNIQUE`, a collision
would abort the fold, and §1.3 makes re-folding both the recovery path and the entire migration
strategy — so a slug collision would render the system unrecoverable and unupgradeable. Fifty bits
makes the birthday bound irrelevant at any volume a single human produces.

**Collisions are still resolved deterministically, not left to chance.** If two slugs collide the
loser appends a further suffix, ordered by `tgt` bytes — a total order every replica computes
identically, so the resolution is not arrival-order dependent and cannot differ across a re-fold.

**Textless captures get a stable prefix rather than nothing.** A voice note has no text at capture
time (§2.2), and images, share-sheet captures and empty captures are the same. Deriving from the
later transcript is forbidden — machine output changes. So the prefix comes from what *is* known
at capture: kind and date, `voice-2026-08-21-<suffix>`. The suffix carries the uniqueness; the
prefix is only there to be readable.

**The derivation is a frozen, byte-specified algorithm**, versioned like the canonical encoding
(§1.3 rule 5) and for the same reason. Slugs are written by hand into other notes' text, so
"improve the stopword list in v2" would silently change existing slugs on the next re-fold and
break every manual link ever written — and with a phone two versions behind a desktop (§1.3),
two devices would compute different slugs from the same log. Specified: lowercase, NFC
normalisation, Unicode word segmentation, first four words over three characters, ASCII
transliteration where a script has a standard one and the raw word otherwise, hyphen-joined.

**A redacted note keeps its row and gets a text-free slug.** This resolves a contradiction an
earlier draft carried: `notes.slug` is `NOT NULL`, the slug derives from capture text, redaction
destroys that text — so after a re-fold the row could not be reconstructed at all, and §4.2's
`[redacted 20 Aug]` marker and §4.3's "a link whose target was redacted says so" both had nothing
to render. Both require the row to survive.

So the fold has a second rule: **if a note is redacted, its slug is `redacted-<suffix>`**, derived
from `tgt` alone, which always survives. Deterministic, reproducible on every re-fold forever, and
it has a useful side effect — the note's own slug stops revealing the words it was built from the
moment you redact it. Copies already written into other notes' text are still beyond reach
(§1.1 limit 5), but the canonical name is not.

### Credential detection, and why it must exist in M1

**Slugs are visible content, and that has a redaction consequence** — see §1.1 limit 5. A slug
derived from `postgres prod password hunter2` becomes readable text inside every note that links to
it, and redacting the target does not reach those copies. The slug is the one part of a note's
content that *escapes into other notes*, so a credential-shaped slug is a leak with no cleanup path.

An earlier draft assigned the detection to the recognizer (§3.1) — which is M2, while the slug
algorithm is frozen in M1. Those two statements are incompatible: adding the check later would
change existing slugs on the next rebuild, which is the exact breakage freezing exists to prevent.
It was written in two sessions and the contradiction went unnoticed.

**So a minimal detector ships in M1, inside the frozen algorithm.** Not the recognizer — a pure
predicate over the capture text, in `capture-core` alongside the other frozen primitives: high-entropy
tokens, long base64 or hex runs, `password`/`token`/`secret`/`key` adjacent to a value, PEM headers,
connection strings. A flagged capture takes a date-based prefix instead of a word-based one.

Two properties make this safe to freeze:

- **It may only ever become more conservative.** A later version may flag *more*, because a note
  already slugged by words keeps the slug it was given — so tightening the predicate never rewrites
  history.
- **False positives are nearly free; false negatives are permanent.** An over-flagged innocuous note
  is slightly less readable. An under-flagged credential is an unfixable leak. Tune it to over-flag.

**The flag does more than shape the slug.** Detection happens once, at capture, and the same signal
is used in two other places where it is equally cheap and equally worth acting on:

- **No pre-redaction snapshot** is written for a flagged note (§1.2). The snapshot exists so a
  misfired redaction is recoverable; for content that is probably a credential, a 30-day plaintext
  copy is the wrong default in the one case you are most likely to be redacting deliberately.
- **Flagged notes are excluded from export by default** (§1.2), so a credential does not propagate
  into offsite backups that redaction can never reach.

**That second rule cuts against "data loss is the cardinal sin", so it carries a rail.** Because the
predicate deliberately over-flags, silent non-export would mean real notes quietly missing from
every backup. So exclusion is **never silent**: excluded notes are surfaced in the health surface
with a count, and clearing the flag on a note is one gesture and puts it in the next export. The
default protects the credential case; the visibility protects the false-positive case.

`redact --with-links` remains the other mitigation. Anything copied by hand into another note is
beyond reach by construction.

An optional **alias** gives a note a chosen name — `[[acme-budget]]` — via an `alias` event, LWW.
Aliases and slugs share one namespace and one uniqueness constraint, so an alias can never shadow
another note's canonical slug and silently hijack links pointing at it.

**Reassigning an alias breaks the links that used it, visibly — which requires storing what the
link meant when it was written.** An earlier draft promised this while specifying resolution as
"alias, then slug, then broken", which does the exact opposite: move `acme-budget` from A to B and
every existing `[[acme-budget]]` quietly resolves to B. Silently pointing an old reference at new
content is how a memory system starts lying to you, so the promise is kept structurally: when a
link is authored, the resolved `tgt` is recorded alongside the raw text in `links`, and resolution
compares the two. Mismatch renders as broken with both names shown, never as a silent retarget.
The UI warns before reassigning and lists what will break.

#### Three kinds of link, kept distinct

| Kind | Written by | Navigates to |
|---|---|---|
| **Entity** — `@sarah`, `^acme-migration`, `#client-acme` | extracted automatically (§3.7) | an entity page |
| **Note** — `[[slug]]` | you, via `[[` autocomplete | that note |
| **Machine** — similarity | the `links` enrichment class (§2.5.1) | shown as *related*, never mixed in |

Machine links stay in their own section. Conflating suggested with asserted means you can no
longer trust either — you would never know whether a connection was your judgement or a model's
guess. A machine link can be **promoted** to an explicit one with a single gesture, which is how
that layer earns trust incrementally rather than demanding it up front.

Promotion writes an `edit`, since explicit links live in note text — so it has two side effects
that must be suppressed rather than inherited: it would trigger a reparse (§3.1.4), which could
silently change the note's type or schedule, and it would bump the note in the timeline (§4.2).
Neither is what "accept this suggestion" means, so a link-promotion edit is marked as such and
does neither.

Entity links carry the load. They exist without anyone linking anything, which is what makes this
usable on day one rather than after months of curation.

#### Entity pages

An entity page is a filtered timeline of everything mentioning it, optionally preceded by a
**backing note** — the note that *is* the project, rather than one that mentions it. Most entities
never have one, and that is the point: pages exist for every person and project immediately, and
gain content only where you choose to write it.

**Entities have identity of their own**, not just a display string. A bare name would mean that
renaming a person silently forks their page into two, that two people called Sarah are
indistinguishable, and that there is no merge path when extraction produces both "sarah" and
"sarah chen". So `entities` carries a stable `ent_id` with a `display` name and a `merged_into`
pointer (§1.4), and binding a backing note sets `note_tgt` on that row rather than adding a value
to `notes.type` — which enumerates `note | task | event | series` and does not include `entity`.

#### The note page

Wiki browsing requires a detail view, which nothing in this document previously specified:

```
budget-40k-sarah-8f2a                              [[acme-budget]]

  budget 40k → 55k, Sarah owns, revisit Q2
  ▸ Tue 18 Aug 14:32 · acme-migration · branch: rework-auth

  people      @sarah
  projects    ^acme-migration
  tags        ⌗client-acme

  links to    [[migration-timeline-1c40]]
  linked from [[q3-planning-77de]] · [[sarah-1:1-notes-0a91]]
  related     [[budget-forecast-5e02]] (0.81) · [[acme-scope-ac4f]] (0.74)   ⟵ suggested
```

Backlinks are one reverse query over `links` — free once the edges are indexed.

Broken links render **as broken**, showing the raw text that failed to resolve. A link whose
target was redacted says so. Same principle as visible redaction (§1.1): an unexplained absence
costs more trust than an explained one.

**A leaf replica must not render "broken" when it simply does not hold the target.** A leaf holds
a recent window (§5.5), so a link to older material resolves to nothing locally — and reporting
that as broken is a false statement of exactly the kind §7 mode 17 forbids. Two things are needed
and neither existed: the link renders as **"not local"**, and the protocol gains a resolution
request, since a leaf holding only `[[slug]]` does not have a `tgt` to ask for and `WANT_TGT` is
keyed by `tgt`. So BSP adds `WANT_NAME { slug_or_alias }`, answered with the target's identity and
events. Without it the wiki layer is unusable on leaf replicas.

Navigation keeps a back/forward history, since click-through is the entire point.

### 4.4 Calendar layout

All-day events render in a separate band above the timed grid —
not stylistic, but a direct consequence of §3.2: they are floating dates and have no position
on an hour axis. The timed grid draws expanded occurrences from the projection, with overlap
resolved by column packing. Occurrences shifted by a DST transition are marked, since a silent
shift is exactly the kind of thing that makes you distrust the whole system. Moved occurrences
render at their new time; cancelled ones are hidden but recoverable, because `occ_cancel` is an
event like any other and nothing is ever actually gone.

### 4.5 The health surface

Eleven places in this document promise something is "surfaced in the UI" — quarantined events,
outstanding redactions, divergences, no-capable-device, leaf coverage gaps, spool warnings,
unexpected `device_add`, parse suggestions, stale calendar mirrors, non-local blobs, export
pruning. Until now there was nowhere for any of them to appear.

**Most of these are not errors, and building a warnings dashboard would be a mistake.** A
quarantined event from a newer client is §1.3 working correctly. A non-local blob is normal. A
leaf not holding old material is its definition. Rendering those as red badges trains alarm
fatigue, and for a user with anxiety a screen of standing unresolved warnings is a permanent cost
with no matching benefit — §2.6 added a notification budget for exactly this reason.

Silence is worse, though: the entire trust argument here rests on things being visible rather
than silently wrong, and §3.1.3 notes that trust once lost does not come back. So the answer is
severity tiering and honest language, not suppression.

#### Tier 1 — needs you

Something is blocked, or data is lost if it is ignored. Rare enough to be worth interrupting for.

- **Disk full, spool in use** (§7 mode 6)
- **A peer not reached in over a week.** Promoted from diagnostics, because it is the leading
  indicator of the failure §1.2 exists to prevent: a phone that has silently stopped syncing holds
  captures that exist on exactly one device. §8.5 predicts this exact case and notes it is
  invisible — without a `MulticastLock`, Android multicast receive "silently does nothing".
- **Rejected events** (bad signature, `bhash` mismatch — §5.3). §7 mode 9 is *forged or corrupted
  event*; burying that in diagnostics while calling an unexpected `device_add` unburiable was
  inconsistent. Malformed events are §1.2's second-worst threat and belong here too.
- A redaction outstanding against a peer that is retired or long gone (§5.4)
- An unexpected `device_add` (§5.3)
- Export pruning required after a redaction (§1.1)

**"Unexpected" is computable**, which it previously was not: `ops.db` records local pairings, so a
`device_add` for a key this device did not pair and did not itself emit is what raises the alert.
Without that record, every legitimate pairing of a new tablet would fire a non-dismissible
security alarm on every other device — training exactly the alarm fatigue this section exists to
avoid.

**Delivery: one notification per day, never on the lock screen — except disk-full, which is
delivered immediately.** A daily cap on the only tier-1 item with a running clock inverts the
tier's own justification: the spool is 10 MB draining toward "capture fails loudly", a bound
measured in captures rather than days.

**Tier 1 items are acknowledgeable, not merely "persistent until resolved".** Two of them can
never be resolved by the system: export pruning happens in external tools it cannot observe, and
an undeliverable redaction stays undeliverable by design (§5.4 deliberately keeps retired peers
listed as never acknowledged, because that is the true statement). Left non-dismissible, half of
tier 1 becomes the permanent standing warning this section opens by ruling out. So they can be
acknowledged — which records that you saw them, and keeps them visible in the status page without
keeping them in your face.

#### Tier 2 — worth knowing

Shown on the **status page**, and additionally folded into the daily review when review is
running.

```
2 notes diverged
3 parse suggestions below threshold
3 recordings awaiting transcription — no capable device
1 external calendar stale since 14 Aug
```

An earlier draft made the daily review the *only* home for tier 2, which coupled a correctness
surface to a therapeutic one in two bad ways: §2.6 guarantees that "pause review" always works
and needs no reason, so pausing it would silently disable health reporting; and review does not
exist until **M5** (§8.10), leaving tier 2 homeless for four milestones. The status page is the
home; review is a convenience.

#### Tier 3 — diagnostic

Never pushed, never badged. On a status page visited deliberately: quarantine counts by reason,
per-peer sync state, leaf coverage gaps, blob locality and cache size, enrichment queue depth.

#### The status page

```
Sync          desktop ✓ 2m ago · phone ✓ 4m ago · laptop — last compared 34 days ago
Storage       events.db 41 MB · blobs 1.2 GB · 3 blobs not local
Enrichment    ✓ nothing pending
Quarantine    2 events from a newer version — will fold after upgrade
Rejected      ✓ none
Redactions    ✓ all delivered
```

**Agreement is stated per peer, with staleness, and never claimed for a peer that has not been
heard from.** An earlier draft printed "✓ all in agreement", which asserts something a replica
cannot know: a version vector proves what a peer held *at the last exchange*. A laptop switched
off for a month would still have shown green. §5.5 and §1 both name this failure — "both sides
report converged; they are not" — so printing an unqualified green is the system telling exactly
the lie it was built to detect. §4.5's rationale for affirmative reporting was that ambiguity is
itself a cost; trading ambiguity for a false green is a worse trade, in the one system whose whole
purpose is compensating for unreliable memory.

Leaf replicas never appear in an agreement line at all, because a version vector cannot express
what a leaf holds (§5.5). They report coverage instead.

**Wording is per reason, not per count.** *"2 events from a newer version — will fold after
upgrade"* is accurate and does not manufacture worry. But that phrasing must not be applied to a
count that includes forged or malformed events — those are tier 1 now, are counted separately as
`Rejected`, and are never described as version skew. Where accuracy and reassurance come apart,
accuracy wins.

#### One live element

A **pending redaction** is time-boxed and actionable, so it gets a live countdown with a cancel
affordance for as long as its window is open (§1.1), then disappears entirely. It is the only
place in the system where a delay *is* the safety mechanism, which is what earns it the only
persistent live element in the interface.

---

## 5. BSP — Brain Sync Protocol

### 5.1 Shape

Two planes, deliberately separated:

- **Event plane** — small, eager, fully replicated. Every full replica holds every event from
  every origin, forever.
- **Blob plane** — large (audio, images), lazily fetched, reference-counted. Constrained devices
  hold blob *references* but fetch bytes only on demand, so a phone never fills with a year of
  audio.

Correctness comes entirely from anti-entropy (§5.5). Live push (§5.4) is a latency optimisation
and is allowed to fail silently — with exactly one exception: `redact`, where delivery latency
is the guarantee rather than a nicety, and which is therefore pushed eagerly and retried until
acknowledged (§1.1, §5.4).

### 5.2 Discovery and transport

**Discovery.** mDNS/DNS-SD, service `_bsp._tcp.local`. The TXT record carries a device
identifier and a short digest of its version vector, so a peer can tell whether it needs to sync
*without opening a connection*. A digest rather than the vector itself, because TXT records are
size-constrained and the vector grows with n.

**Both fields are rotated, because mDNS is broadcast in the clear to everyone on the network.**
A stable device id advertised continuously on every network you join is a cross-network
identifier that links your phone, laptop and desktop to each other at every café, office and
airport — to exactly the guest phone and housemate's laptop named as the threat model in §5.3.
And a plain vector digest changes on every append, making it an activity oracle: a passive
observer learns when and how often you capture ("14 notes during that meeting") without ever
completing a handshake. So the advertised identifier is a per-network rotating pseudonym derived
from the roster key, and the digest is keyed rather than plain. Roster members can recognise each
other; nobody else can correlate or count.

mDNS is blocked on many corporate and guest networks, so there is a static fallback: last-known
addresses cached from previous sessions, with jittered exponential backoff.

**Transport.** TCP with the **Noise_IK** handshake pattern: the initiator already knows the
responder's static public key, and transmits its own inside the first message. Mutual
authentication and forward secrecy with no PKI, no certificates, and no CA to operate. A device
not on the roster cannot complete the handshake and cannot read, inject, or observe anything
beyond connection metadata.

**Why IK and not KK, which this section originally specified.** KK requires *both* statics to be
known before the handshake begins. That is true of the side that dials — it chose who to call —
and false of the side that answers, which has a TCP connection and no idea who opened it. It
cannot ask, because the rule below forbids any message outside an established session, and it
cannot try each member in turn, because a failed attempt consumes the connection. **KK is not
implementable for an inbound connection in this topology**, which was discovered by writing the
daemon rather than by reading this document.

IK keeps the property that mattered and moves the one that did not. The responder's X25519 key
lives in the log, so a device without the archive cannot produce a valid first message and cannot
begin. The responder learns the initiator's key from the handshake and checks it against the
roster.

The honest cost: a **retired** device still holds the log, so it completes the handshake and is
refused on the roster check rather than being unable to start. §5.3 already requires that check —
retirement tears down live sessions and refuses new ones — so the mechanism exists; under IK it is
load-bearing rather than belt-and-braces. A device that was never a member is still unable to
begin.

**Everything on the wire is inside the Noise session. No exceptions.** Stated explicitly because
it is otherwise easy to violate by accident — the blob plane moves large byte ranges and it is
tempting to serve those over a plain socket or a small HTTP endpoint for simplicity. That would
put your audio recordings in cleartext on the LAN while the text that describes them is
encrypted. Every message type — `EVENT`, `REDACT`/`REDACT_ACK`, `HELLO`, `RANGE`, `WANT`/`BLOB`,
`WANT_TGT` — is a Noise transport message inside an established session:

```
TCP connect → Noise_IK handshake (mutual auth, forward secrecy)
            → every subsequent frame encrypted and authenticated
```

There is no unauthenticated pre-handshake message, and no fallback to plaintext if the handshake
fails — a failed handshake closes the connection.

**Framing.** Noise transport messages cap at 65535 bytes including the 16-byte AEAD tag, so the
usable payload is 65519. Blob chunks are therefore **32 KiB**, which leaves comfortable room for
framing overhead and keeps a chunk well inside one message. (An earlier draft wrote a 65536-byte
blob chunk, which does not fit in a Noise message at all.) Sessions rekey on the standard
interval so nonces cannot be exhausted on a long-lived link doing a large transfer.

**What this does not hide.** Encryption protects content, not the existence of traffic. An
observer on the LAN still sees that two hosts are talking, roughly when, and roughly how much —
so capture timing and volume remain inferable from packet sizes and timing even though nothing
readable leaks. That is the same exposure any LAN sync tool has, it is not worth padding traffic
to defeat for this threat model, and it is stated rather than left to be discovered. The mDNS
mitigations above address the cheaper version of the same leak.

### 5.3 Membership and trust

This is the part that has to change for n devices, and it is security-relevant — worth reading
carefully rather than skimming.

Pairing every device with every other device is O(n²) manual QR scans and does not scale past
about four devices. Instead, **the roster lives in the log**.

Each device generates an Ed25519/X25519 static keypair on first run. A new device is paired
**once, out of band, with any single existing device**.

Pairing must be a **mutual** exchange, not a one-way scan. Even under Noise_IK the initiator must
know the responder's static public key before the handshake begins — so a newcomer that has only
published its own key knows no peer keys, cannot complete a handshake with anyone, and therefore
cannot read the roster, which lives in the log, which is only reachable through a handshake. A
one-directional QR flow deadlocks.

**What was built instead of the QR exchange below.** The desktop has no camera, so the gesture is
six digits shown on one device and typed into the other, with **SPAKE2** turning that code into a
strong key and a `Noise_NNpsk0` handshake confirming it. Six digits used directly as a key would
be a million guesses tried offline in seconds by anyone who recorded the exchange — and the
handshake would confirm the right guess for them. With a PAKE a wrong code yields a different key
and no information about the right one, so guessing has to be online, one attempt at a time. The
mutual-exchange requirement below is unchanged; only the channel that carries it differs.

```
1. newcomer displays QR: its public key + a short-lived pairing token
2. existing device scans it, then displays a QR in return:
   its own public key, the roster snapshot, and a MAC over both keyed by the token
3. newcomer scans that, verifies the MAC, and now holds a peer key + the roster
4. existing device emits a signed device_add; it replicates as any event does
```

The pairing token is what binds the two directions together: it is generated by the newcomer,
displayed only on its screen, and used to authenticate the reply — so an on-LAN attacker cannot
substitute their own roster and capture the new device into a different mesh. Step 2 is the step
whose absence made the original flow unimplementable.

One physical scan pair, not n handshakes. And because step 2 does not require a signature from
any existing roster member, this is also the recovery path when the log-based roster has been
poisoned by a bad retirement.

Trust rules, single-user, stated explicitly:

- Any roster member may add or retire another device. There is no admin device, because a
  designated admin would be a single point of failure — exactly what "serverless" is meant to
  avoid.
- `device_add` and `device_retire` are signed by the emitting member. Unsigned or
  wrongly-signed membership events are quarantined, never folded.
- A retired device's **past events remain valid** — they were legitimately authored, and
  discarding them would mean losing real notes.
- Concurrent `device_add` for different devices → union, both admitted.
- Concurrent `device_add` and `device_retire` for the *same* device → **retire wins**. This is
  deliberately the opposite of the add-wins rule used for tags (§6): for content, losing data
  is the worse outcome, so adds win; for access, granting unintended access is the worse
  outcome, so revocation wins.
- Concurrent `device_retire` for the same device naming **different heads** → **the lowest head
  wins**, per origin. Two devices retiring a stolen phone will name whatever seq each had seen,
  and without a rule the two replicas permanently disagree about which events exist — with no
  divergence flag, since `divergences` tracks note bodies only. Lowest-head-wins is chosen over
  latest-HLC because it is the safe direction: it excludes the events whose provenance is in
  doubt. The cost is that a legitimate late capture on a lost-then-found device may be excluded;
  it remains in the log, quarantined and visible, and a deliberate reissue naming a later head
  re-admits it.

**Membership is keyed by the device's public key, not by its origin id.** `dev` is
`H(pubkey ‖ incarnation)` (§1), so a restore mints a new origin — and if the roster were keyed by
origin, that restored device would have no `device_add`, every peer would quarantine everything
it writes, and restore would be a lockout on the path most likely to be used in an emergency.

So `device_add` binds **key → {origins}**, and a device may append a new origin under its own
existing key by emitting a self-signed origin claim, which every peer can verify against the
roster key it already holds. Correspondingly, **`device_retire` retires the key**, carrying a head
per known origin. Retiring only an origin would let a stolen device mint a fresh incarnation and
walk straight back in — which would have made §7 mode 14's entire remedy evaporate on one restart.

**Retirement names a head, not a timestamp.** The obvious rule — reject events the retired device
authored "after" the retirement — cannot work, because the retired device chooses its own `hlc`
and can simply backdate. There is no causal ordering between a retirement and the events of the
device being retired.

`device_retire` therefore names the retired origin's **last legitimate `(seq, id)`**, taken from
the issuing device's view. The per-origin hash chain (§1) makes that enforceable: every event
carries `chain`, the id of its predecessor, so any event beyond the named head is provably
outside the retired history no matter what timestamp it claims. If the retired device has already
propagated events past the named head to some replica, the retirement can be reissued naming the
later head — an explicit decision about what to keep, rather than a silent race.

**Retirement tears down live sessions.** The roster is checked at handshake and at ingest, which
leaves an open Noise session established one second before the retirement quietly delivering
events afterwards. On ingesting any `device_retire`, a device drops existing sessions with that
origin immediately and refuses new ones.

**Two kinds of failure, two different mechanisms.** An earlier draft applied one rule —
"quarantine everything, advance `heads`" — to every ingest-time check. That was wrong, and
dangerously so, because it let an **unauthenticated** event claim a sequence position:

```
t0  desktop heads[phone.x] = 15677
t1  any compromised roster peer pushes { dev: phone.x, seq: 15678, sig: garbage }
t2  desktop quarantines it, ADVANCES heads to 15678, and relays it onward (§1.3 rule 4)
t3  every replica does the same. Mesh poisoned in O(log n) rounds.
t4  the genuine phone.x:15678 arrives → UNIQUE violation, ids differ → refused
    everywhere, forever, while heads says the position is held
```

A real capture is permanently unreachable, and the old ingest-time rejection made this
impossible. The distinction that resolves it:

| Failure | Authentic? | Handling |
|---|---|---|
| Bad or missing signature; `blake3(body) ≠ bhash`; unknown origin key | **no** | **`rejected` side table, keyed by `id` alone.** Never occupies `(dev, seq)`. **Never advances `heads`.** Never relayed. |
| Unknown event kind; malformed body; post-head from a retired origin; not-yet-admitted origin | yes — signature verifies | In-stream quarantine: stored at its position, `heads` may advance, **relayed**, excluded from the fold |

Only the second class may be quarantined in-stream, and only because those events are provably
authored by the origin whose position they occupy — so no one else can be squatting it. The
gap-free argument below applies only to them.

**For authentic events, rejection is never used — quarantine is.** A version vector cannot
represent a hole: if a replica refuses event 9350 but accepts 9351, its `heads` cannot advance
past 9349 and anti-entropy re-requests the same range from every peer forever. §1.1 names this
pathology exactly. Authentic-but-unfoldable events are therefore stored, counted, and reviewable;
never silently dropped, never leaving a gap.

**Every quarantine reason has a re-evaluation trigger.** An earlier draft named only "upgrades
unquarantine" (§1.3), which strands the most common case: a newly paired device's first captures
race its own `device_add` through gossip, get quarantined as not-yet-admitted, and — with nothing
to re-examine them — never fold, on a device whose `heads` says it holds them.

| Reason | Re-evaluated on |
|---|---|
| `unknown_kind`, `malformed` | client upgrade; any re-fold |
| `not_admitted` | ingest of any `device_add`; any re-fold |
| `post_retire` | ingest of any `device_retire` naming a later head; any re-fold |
| `chain_gap` (predecessor not yet held) | arrival of the predecessor |

Re-evaluation also runs on every re-fold, so a rebuild is a general repair for anything missed.

**Lockout protection.** Because any member can retire any member, one compromised or malfunctioning
device could retire every other device, after which no honest device can re-admit anything —
`device_add` requires a signature from a current member. Three guards:

- A device **refuses to apply a retirement of itself** that it did not issue, unless confirmed
  out of band on that device. A stolen device is unaffected by this, since the point of retiring
  it is that its peers stop accepting its events, which they still do.
- A retirement that would empty the roster is refused locally.
- **Re-admission by physical possession always works — via a co-signed event, not via pairing
  alone.** An earlier draft claimed the QR flow was the recovery route, which was wrong: pairing
  bootstraps *transport* keys, but the roster entry that actually gates ingest still came from a
  member-signed `device_add`, and in a lockout every honest device has been retired and is
  therefore not a member. Both would sit there able to talk and unable to accept each other.

  So there is a distinct kind: **`device_readmit`, signed by two keys**, valid even when both
  signers are retired. Forging one requires compromising two devices *and* being in the room with
  them, which is a materially higher bar than the single compromised device that caused the
  lockout. It is the only event in the system whose validity does not descend from current roster
  membership, and that exception is stated here rather than left implicit.

  Single-device systems cannot use this — with one device there is no second signer. That case is
  covered instead by a device refusing to apply a retirement of itself that it did not issue.

**Per-event signatures are mandatory and independent of transport.** Events are relayed — the
desktop will hand you events that originated on the phone — so transport security alone proves
only "the desktop sent this," never "the phone wrote this." Every event is verified against its
origin device's roster key on ingest. Verification failures are quarantined and logged, never
folded into state.

**The limit, stated honestly.** Because any member can admit a device, a fully compromised
device can admit an attacker. Nothing in this design prevents that, and it would be dishonest
to imply otherwise. Detection is that `device_add` events are visible in the log and surfaced
in the daily review, so an unexpected admission is *noticed* — but recovery from a compromised
device means retiring it and re-keying the roster, not silent repair. That tradeoff is
acceptable for a single-user home LAN and would not be acceptable for a multi-user system.

The threat model worth designing against here is not a targeted attacker; it is **a guest
phone, an IoT device, or a housemate's laptop on the same wifi**, which is an ordinary
situation rather than a hypothetical one. Roster-gated Noise_IK handles that case completely.

### 5.4 Live push

When a device appends locally, it immediately writes the event to every currently-open peer
session:

```
→ EVENT { …envelope… }
```

Fire-and-forget. No acknowledgement, no retry, no queue. If the peer is asleep, off-LAN, or the
socket is broken, the event is simply not delivered now, and anti-entropy will deliver it later.

This separation is intentional: push carries no correctness burden, so it can be as dumb and as
fast as possible. Typical LAN delivery lands well under 50 ms, which makes captures feel live
across devices without a single line of retry logic.

**Eager push — `redact` only.** One event kind does not follow the rule above:

```
→ REDACT { …envelope… }
← REDACT_ACK { id }
```

A `redact` is pushed immediately to every *known* peer, not merely to open sessions; peers that
are not connected are dialled. Delivery is retried with exponential backoff until each roster
member acknowledges, and unacknowledged peers are listed in the UI so "gone everywhere" is an
observable state rather than an assumption. The queue survives restarts.

**Acknowledgement is tracked per origin, not per current roster member** — otherwise the most
important sequence in practice reports false success. The realistic order of events is: phone is
stolen → you retire it → *then* you redact the sensitive notes. If outstanding peers were read
from the current roster, retiring the phone would remove it from the set and the UI would report
complete propagation for a device that never received anything. Instead the pending set is fixed
when the redact is issued and retired devices remain listed as **never acknowledged**, which is
the true and useful statement.

This is the only place in the protocol where push carries a correctness burden, and it is worth
the asymmetry: for every other event, arriving a few hours late costs nothing, whereas for a
redact the delivery window is exactly the window in which the content can still be read.

### 5.5 Anti-entropy — the actual correctness mechanism

Because events are single-writer-per-origin and `seq` is gap-free, a replica's entire state is
described by a **version vector**: one integer per origin device, read straight off the `heads`
table.

```
{ "desktop": 41822, "laptop": 9310, "phone": 15677 }
```

On session establishment:

```
A → B   HELLO   { vv: {desktop: 41822, laptop: 9310, phone: 15677} }
B → A   HELLO   { vv: {desktop: 41500, laptop: 9310, phone: 15680} }

# each side computes the diff itself — no negotiation round needed
A → B   RANGE   { dev: "desktop", from: 41501, to: 41822 }
B → A   RANGE   { dev: "phone",   from: 15678, to: 15680 }
```

That is the whole reconciliation algorithm. No Merkle tree, no bloom filter, no set-difference
estimator. The single-writer-per-origin decision from §1 is what buys this: the complexity that
normally lives in a sync engine has been moved into the data model, where it costs nothing.

Anti-entropy runs on session open, on network change, on wake, and on a 30-second timer while
connected.

**Peer selection at n devices.** A full mesh is O(n²) sessions and is the wrong shape past a
handful of devices. Instead, each round a device syncs with **a small random subset of
reachable peers** (fanout 2–3). This is standard epidemic gossip: an event reaches every
connected replica in O(log n) rounds with no coordination, no topology to configure, and no
election. At n = 3 it degenerates to a full mesh anyway, so nothing is lost in the small case.

Any device relays any origin's events. If the desktop is powered off, the laptop still receives
the phone's events, and the desktop catches up from whichever peer it meets first. **No device
is required to be online for the system to converge** — this is what makes it genuinely
serverless rather than "one machine is quietly the server."

**Keeping the vector bounded.** Every origin that ever existed keeps an entry, and each restore
or reinstall mints a new incarnation (§1), so the vector grows with origin *history*, not device
count.

Sealing collapses the bookkeeping — but it must not collapse the **completeness oracle**, which
is the mistake in the obvious version. If a retired origin's entry simply disappears from the
vector, a newly paired device asking "what am I missing" gets no answer for that origin and
silently receives none of its events, while its `heads` cheerfully reports agreement with every
peer. After two reinstalls a fresh device holds a fraction of the archive and believes it is
complete. Worse, an unreachable redaction can then be permanently bypassed: a returning old
replica hands the new device content whose destroying `redact` lived in a sealed stream the new
device never received.

So a sealed origin **keeps a one-line entry recording its final `seq` and the id of its final
event**, permanently:

```
{ "desktop.3": 41822, "phone.1": 15677, "sealed": { "laptop.1": [9310, "b3:ac4f…"] } }
```

What sealing actually saves is the *live* reconciliation work, not the entry itself — a sealed
origin is never diffed or re-requested once both sides show the same final id, and a device
holding it can answer a newcomer's request for the whole stream in one range. The honest claim
is that the vector grows by ~40 bytes per retired origin forever, which for a single user is a
few kilobytes over a lifetime. The earlier claim that it collapses to "live devices plus a
constant" was wrong, and the correction is cheaper than the bug it prevents.

Sealing requires unanimity among current roster members, which is a small consensus over a
concurrently-mutable set. To keep that safe, a seal names the roster it was computed against and
is only honoured by devices that agree that roster is current; a device admitted concurrently
simply causes the seal to be recomputed rather than silently missing data.

**Device roles.** A device joins as `full` or `leaf`. Full replicas hold all events — the
default. Leaf replicas (a rarely-used tablet, a work machine you would rather not carry the
archive on) hold a recent window plus anything explicitly pinned, and fetch older material on
demand.

**A leaf cannot be described by a version vector**, and pretending otherwise is a real hazard: a
leaf holds a *suffix with holes*, not a prefix, so advertising `desktop.3: 41822` while holding
only 41000–41822 tells every peer "the leaf needs nothing" — harmless until that leaf is promoted
to `full`, becomes the last surviving replica, or seeds a new device. Leaves therefore advertise
an **interval set per origin**, not a single integer, and are flagged as partial in `HELLO` so no
peer ever treats one as a seeding source without checking coverage. Promotion to `full` is an
explicit operation that backfills the gaps first.

Leaves also need a fetch that is not keyed by `(dev, seq)` — a leaf that lacks a note cannot know
which sequence ranges contain its events, since that is precisely the index it does not have. So
the protocol adds `WANT_TGT { tgt }`, answered with every event targeting that note. Leaves do
not relay, and are excluded from the sealing quorum so an offline tablet cannot block it.

### 5.6 Blob plane

```
A → B   WANT { hash: "b3:9d1e…", have_bytes: 0 }
B → A   BLOB { hash: "b3:9d1e…", offset: 0, len: 32768, data: … }
```

Like every other message type, these travel **inside the Noise session** (§5.2) — the blob plane
is not a separate unencrypted byte-serving path, which is the easy mistake here. Chunks are
32 KiB to fit inside a single Noise transport message.

Range-resumable, so an interrupted 40 MB audio transfer resumes rather than restarts. Content
addressing means verification is free and duplicates are impossible.

#### Three different operations, previously conflated

| Operation | Scope | Recoverable |
|---|---|---|
| **Evict** | one constrained device drops local bytes | yes — refetch from any full replica |
| **Expire** | every replica drops it | **no** |
| **Redact** | every replica, immediately, permanently (§1.1) | no, deliberately |

Collapsing these into one word is how "freed some space" silently becomes "lost the recording."
They are kept distinct in the schema and in the UI.

**Eviction — aggressive, and safe.** Constrained devices (`leaf` replicas, the phone) evict on
an LRU with a size cap and a 30-day default. Nothing is lost: the bytes are refetched on demand
from any full replica. This is where the actual storage pressure is, and it costs nothing to be
liberal about it.

**Global expiry: removed.** Earlier drafts expired blobs after 90 days if transcription had
succeeded. That is now cut entirely, for three reasons that each independently sink it:

1. **It was not actually deterministic.** The retention window was a `setting` — LWW and
   eventually consistent — so replicas would apply different windows at different moments and
   destroy different sets of bytes, with no event and therefore no audit trail. One mistaken
   `setting` would have destroyed every blob on every replica silently.
2. **It races pin state.** Pin a recording on the phone on day 89 while offline; the desktop
   deletes it on day 90 having not yet seen the pin. Every replica then converges on "pinned",
   with the bytes existing nowhere. Convergence after an irreversible act is not convergence.
3. **Gating destruction on a derived artifact is unsound.** "Transcription completed
   successfully" is asserted by whichever device ran it, and nothing verifies it. A transcriber
   that returns an empty string for a quiet recording satisfies the gate and the only copy of
   the audio is destroyed everywhere. That is a bug amplifier, not a policy.

Eviction already solves the real problem — the phone — non-destructively. At realistic volumes
(roughly 2 GB/year even with heavy voice use, once transcripts are counted properly) keeping
everything on a machine with a disk costs little, and discards nothing: tone, hesitation, and
whatever the transcript got wrong.

**Redaction is the only way a blob is destroyed**, and it is explicit, bounded, and reversible
for the cancellation window (§1.1). Dropping the last reference to a blob overwrites and unlinks it.

The desktop's policy otherwise: fetch everything, it is the archive of record.

---

## 6. Conflicts

### Level 1 — the event log: no conflicts possible

The set of event *identities* is grow-only. Duplicate delivery is idempotent because events are
keyed by their envelope hash. Out-of-order delivery is fine because state is a fold over the
HLC-sorted union, not a function of arrival order. Nothing to resolve, by construction.

**Precisely: this is a join-semilattice, not a pure G-Set** — and the distinction matters,
because an earlier draft claimed the latter while specifying mechanisms that remove things.

The lattice is: identities only ever accumulate, and each event's body takes a value from
`{content, ⊥}` where `⊥` (redacted) is **absorbing**. Redaction does not remove a set element —
the row survives with `id`, `bhash`, `chain` and `sig` intact and still verifiable — it moves one
element to a bottom value nothing can lift. Join stays commutative, associative and idempotent.

**Two honest qualifications, because `redact_cancel` exists:**

- **⊥ is only absorbing once the cancellation window has closed.** During the window the value is
  `queued`, and a cancel returns it to `content` — an upward move, which a semilattice does not
  permit. What actually converges is the state *after* the window: the redact's HLC fixes the
  deadline identically on every replica (§1.1), so all replicas compute the same final value from
  the same event set, with no dependence on when each one happened to receive anything.
- **The derived view converges; physical destruction is best-effort.** If a cancel reaches replica
  A before its deadline and replica B after, both agree the note is `content` — but B has already
  destroyed the bytes and reports the loss rather than hiding it. This is stated plainly because
  the alternative reading, that a cancel always restores content everywhere, is not true and
  would be relied upon.

Two further provisos, handled elsewhere rather than assumed:

- Redactions must be **retained indefinitely**, or a stale replica reintroduces content and the
  bottom value is not absorbing after all (§1.1).
- Redactions must remain **reachable through anti-entropy**, which is why sealing keeps a
  permanent entry per retired origin instead of dropping it (§5.5).

The other two removal mechanisms are gone: global blob expiry was cut (§5.6), and ingest-time
rejection was replaced with quarantine, which stores the event and advances `heads` (§5.3).
Quarantine is a fold-time exclusion, not a set removal, so it does not touch the lattice.

### Level 2 — derived state: conflicts possible, rules are per-type

Conflicts exist only in the projection, and only when two devices mutated the same thing while
partitioned.

| Type | Rule | Rationale |
|---|---|---|
| Note body | **Both retained.** HLC winner becomes current; loser attached as a `divergence` and flagged. | Data loss is the cardinal sin in a memory system. Nothing is ever silently dropped. |
| Tags | OR-Set union, **add-wins**. `untag` removes only the specific `tag` events it observed. | A concurrent add should survive a remove that never saw it. |
| Scalars (status, due date) | LWW by HLC; prior value kept in history. | Cheap, and genuinely low-stakes. |
| Delete vs edit | **Edit wins.** The tombstone is retained but does not apply. | Accidental loss is worse than a stale note surviving. |
| `redact` vs anything | **Redact always wins, at any HLC, forever.** It is not resolved at fold time — it is applied destructively at ingest, and retained to destroy matching events that arrive later. | The only rule in the system that deliberately destroys data, and the only one that must not be overridable by a concurrent edit. An edit racing a redact must not resurrect the content. |
| Task state (open/done) | LWW by HLC; prior states kept in history. | Genuinely low-stakes and always visible — you can see the current state and flip it. |
| `occ_done` / `occ_undone` on the same occurrence | LWW by HLC. Two `occ_done` for the same occurrence are idempotent; an `occ_undone` can supersede a completion. | Completing twice is completing once — but un-completing a mistaken tick has to be possible, so this cannot be a pure union. |
| `promote` (type assignment) | LWW by HLC **among same-authority events only**. A human `promote` (`pinned: true`) always beats a parser-authored one regardless of HLC. | Plain LWW is wrong here: the parser will improve and reparse the backlog, and a later machine parse would silently revert a correction you already made. Losing trust in the system that way is unrecoverable. |
| Series rule edited concurrently | **Both retained**, same as note body: HLC winner applies, loser flagged. | A silently merged recurrence rule is very hard to notice and quietly corrupts every future occurrence. |
| Occurrence exception vs series edit | **The exception wins for its occurrence.** The series edit applies to all others. | Specific beats general. This is the intent in every case where both were authored deliberately. |
| `occ_cancel` vs `occ_move` | **Move wins**; the cancel is retained but does not apply. | Consistent with edit-beating-delete. A resurrected occurrence is visible and one gesture to dismiss; a wrongly-cancelled one is invisible and silently missed. |
| Series split vs edit to the old rule | **`prev` decides**: the edit applies to whichever series version the editor actually had materialised. | Comparing an edit's HLC against the split's *occurrence boundary* compares an authoring instant to a calendar instant — they are not commensurable, and the result misfiles edits. If you open the series on 2 Aug showing the August rule and change the time, that edit belongs to August, whatever a September boundary says. `prev` already records exactly which version you were looking at. |
| `device_caps` vs `device_caps` | LWW by HLC, per origin. A device may only author its own. | Capability is a self-assertion about the present; the newest one is the truth. |
| `setting` vs `setting` at equal HLC | Origin id breaks the tie, as everywhere else. | HLC includes the origin precisely so total order is total. |
| `intent` edited concurrently | Same as note body: both retained, HLC winner active, loser flagged. | A silently merged cue fires at the wrong time forever, which is worse than being asked once. |
| `enrich` vs `enrich`, same `(capture, job_class)` | Lower rendezvous rank wins, computed against the capable set **as of the capture's HLC** (§2.5.3). | Ranking against the *current* roster would make re-folding the same log produce different transcripts on different days. |
| `capture` vs `redact` | Redact wins even when it arrives first: the redaction is retained and destroys the capture on arrival. | Stickiness (§1.1). Order of arrival must not determine whether content survives. |

### Detecting real concurrency

The `prev` field does the work, but the rule is **reachability through the `prev` chain**, not a
pair of equality checks. Two `edit` events A and B on the same note:

- **B is reachable from A** by following `prev` links (`B.prev == A.id`, or `B.prev` leads back
  to A through any number of hops) → B saw A. Sequential. Apply in order. Not a conflict.
- **Neither is reachable from the other** → genuinely concurrent. Divergence recorded.

An earlier draft used only the two single-hop tests `B.prev == A.id` and `A.prev == B.prev`,
which miss the most likely real case — the one §6 itself describes:

```
v0 ← A ← B      (two edits on the phone, on the train)
v0 ← D          (one edit on the desktop)
```

Comparing B and D: `B.prev` is A, not D's id; `D.prev` is v0, not B's id; and `B.prev ≠ D.prev`.
Neither single-hop rule fires, so the design defined no outcome for a multi-edit branch against
a single-edit branch. The equality tests also produce false positives in the other direction: a
second edit branched from a superseded version — which a `leaf` can materialise after a partial
fetch, and which happens whenever you deliberately edit the older side of a shown divergence —
looks concurrent when it is not.

Reachability needs a walk, which is why the per-note version graph is materialised in `state.db`
alongside the fold. `tgt` indexing (§1) still avoids scanning the whole log; the walk is over one
note's edits, which is a handful of rows.

Getting this right matters beyond correctness: the divergence count shown in the daily review is
wrong in both directions under the old rule, and §3.1.3 identifies trust in the tool's accuracy
as the thing that is unrecoverable once lost.

### What you actually see

Genuine concurrent edits, single user: realistically a handful per year, and
mostly self-inflicted (editing on the phone on the train, then on the desktop before the phone
reconnects). They appear as one line in the daily review — "2 notes diverged" — with both
versions shown side by side. No modal dialog, no blocking, no interruption at capture time.

---

## 7. Failure modes

| # | Failure | Handling |
|---|---|---|
| 1 | **Device off-LAN** (phone at the office) | Capture appends locally and returns. Network is never in the capture path. Syncs on rejoin. This is the single most important failure mode and it is handled by making it a non-event. |
| 2 | **All peers offline** | Same. Local append always succeeds. A device alone for a month is fine. |
| 3 | **Crash mid-append** | SQLite WAL handles torn writes: the transaction either committed or it did not. `seq` allocation, the row insert and the `heads` update are **one transaction** — otherwise a crash between them produces exactly the gap that halts anti-entropy. Capture commits with `synchronous=FULL` before acknowledging, so a capture the UI accepted is never a lie. |
| 4 | **Clock skew** | **Reported, never enforced.** On ingest a device follows the standard HLC receive rule and **adopts** the incoming timestamp, because that adoption is what preserves causality (§1). Adoption can leave the logical clock ahead of the wall clock; that lead is a tier-1 health item, and authoring continues regardless. Refusing to emit while skewed was the original rule and is rejected: it lets one machine's wrong clock close the capture path on every other machine, which contradicts §0. Events remain folded everywhere, so the projection stays a pure function of the event set. |
| 5 | **Concurrent append on n devices** | Structurally impossible to conflict — disjoint `(dev, seq)` spaces, and no device ever writes another's origin. |
| 6 | **Disk full** | A 10 MB **plain preallocated file** — not a ring buffer (which by definition overwrites its oldest entries and so cannot honour "never drop") and **not a SQLite table** (which would need the very disk that just filled, and whose free-page reservation trick conflicts with the `secure_delete` used by redaction). Written full at install; length-prefixed records appended. `seq` is allocated at **drain** time, not spool time, so a partial or out-of-order drain cannot leave a gap in the origin's stream. Spooled events carry their true capture time and timezone; their HLC is assigned at drain and may lag. At ~200 bytes an event this holds on the order of 50,000 captures, which is far more headroom than needed — the actual protection is the immediate tier-1 notification the moment the spool is in use (§8.7), not capacity. If the spool itself fills, capture fails loudly rather than discarding silently. **Voice capture would exhaust any spool size immediately; when audio lands it needs its own answer, either a blob spool or refusing audio capture while the disk is full.** |
| 7 | **Thundering herd on network rejoin** | Digest-first (version vector before data), jittered backoff, one sync session per peer pair, range requests capped and pipelined. |
| 8 | **Non-roster device on the LAN** | Cannot complete Noise_IK — the responder's static key lives in the log, so it cannot even produce a valid first message. Sees that traffic exists and roughly how much, and nothing readable — every message type including blob transfer is inside the session (§5.2). No TOFU, no auto-pair, no plaintext fallback on handshake failure. |
| 9 | **Forged or corrupted event** | Per-event Ed25519 signature verified on ingest, independent of transport. Failures quarantined and logged, never folded. |
| 10 | **Long partition (weeks)** | Converges to the union on reconnect. Only same-note concurrent edits need arbitration (§6). |
| 11 | **Enrichment failure** (transcription crashes, model unavailable) | Non-fatal by design. The capture stands alone; enrichment retries later. Never blocks, never discards. |
| 10a | **Designated notifier is dead or out of battery** | The ordered fallback list fires after a grace window (§2.4.1). Missing a nudge entirely is the failure that matters; a late one is not. |
| 10b | **Partition makes two devices both fire an intent** | Two notifications. Accepted deliberately — a duplicate nudge beats a missed one. Both `fire` events are recorded, and the projection scores the occurrence as fired once. |
| 10c | **Local-only cue on a device that is not the notifier** | Fires locally regardless. Geofences, app focus and branch cues are only observable where they happen, and relocating them would defeat point-of-performance delivery. |
| 11a | **Assigned device sleeps or dies mid-job** | Self-healing: no result appears, so the next rendezvous rank runs after its delay (§2.5.3). No claims or leases to strand. |
| 11b | **Partition causes duplicate enrichment** | Both results are kept in the log; the projection takes the lower rendezvous rank deterministically. Wasted CPU, no divergence, no human involvement. |
| 11c | **No device can run a job class** (no transcription model anywhere) | Tier 2 in the health surface (§4.5): one line in the daily review, with the reason stated. Never left silently undone, never escalated to a notification. |
| 11d | **Device declares a capability it cannot deliver** | Capability requires the model to be present, not merely adequate hardware. A device that cannot deliver must not rank in the first place (§2.5.2). |
| 12 | **Projection corruption, or a projection schema change** | `state.db` is disposable. Delete and re-fold `events.db`. This is also the entire migration strategy (§1.3) — there is no migration code, only re-folding. |
| 12a | **Event kind or field the client does not understand** | Quarantined: stored, verified, `heads` advanced, **relayed onward**, excluded from the fold. Unquarantines and folds after an upgrade. A device is never refused sync for being too old or too new (§1.3). |
| 13 | **Blob evicted locally** (phone LRU, desktop off) | The note still renders with transcript and metadata; audio shows as "not local," fetched when a peer holding it appears. Graceful, not an error — eviction is not deletion. |
| 13a | **Blob destroyed by age or policy** | Cannot happen. Global expiry was removed (§5.6); the only path that destroys a blob is an explicit `redact`, which is scope-capped and cancellable for the window. |
| 14 | **Device lost or stolen** | Retire it from any other device. Its past events stay (they are real notes); anything beyond the head named in the retirement is excluded by the hash chain regardless of claimed timestamps, and live sessions with it are torn down (§5.3). Data on the lost device itself is only protected by that device's own disk encryption — BSP cannot revoke what has already been replicated. Strengthening this (at-rest encryption, app lock, expiry wipe) is deferred as a cluster in §9.14. |
| 15 | **Device reinstalled** (fresh key, same hardware) | Joins as a genuinely new origin. The old origin is retired and sealed once peers confirm they hold its full stream, so the version vector tracks live devices plus a constant rather than lifetime device history. |
| 16 | **Retired device still running and appending** | Events beyond the head named in `device_retire` are excluded by the hash chain regardless of claimed timestamps, and are quarantined rather than rejected so no `seq` gap forms. Live sessions with it are torn down on ingest of the retirement (§5.3). |
| 16a | **One device retires all the others** | A device will not apply a retirement of itself that it did not issue without out-of-band confirmation; a retirement emptying the roster is refused; and QR pairing re-admits by physical possession without needing a current member's signature (§5.3). |
| 17 | **Leaf device asked for old material it does not hold** (tier 3, §4.5) | Range-fetches from any full replica on demand. If none is reachable, the UI shows the gap explicitly rather than presenting a truncated history as if it were complete. |
| 18 | **Partition splits the roster** (two subsets each add a device) | Both admissions survive the union; the roster converges to the superset. Only a concurrent add/retire of the *same* device needs arbitration, and retire wins (§5.3). |
| 19 | **Misparse on promotion** ("dentist thursday" becomes the wrong Thursday) | Never destructive — the original capture text is the durable record and `promote` only annotates it. Correct from any device; the correction is another event. |
| 20 | **Parse produces nothing** | It stays a note. Fully captured, searchable, surfaced. Parsing is an enhancement, never a gate (§3.1). |
| 20a | **False positive** ("the march timeline slipped" reads as a date) | Promotion is additive: the text is untouched and the note is intact, just wrongly labelled. Below the confidence floor nothing is promoted at all — it becomes a one-line suggestion in review (§3.1.3). |
| 20b | **Parser improves; backlog reparsed** | Reparse emits new `promote` events, but human-pinned corrections are never overridden (§6). Improvement is safe to run over history. |
| 20c | **No parser-capable device reachable for a long period** | Captures accumulate as plain notes with suggestions unresolved. Nothing is lost or blocked; they promote whenever a capable device next appears. |
| 21 | **Timezone database disagrees between devices** | Occurrence expansion lives in `state.db`, per device, and is derived. A tz update is resolved by re-folding, not by data migration. Instants and the origin tz id in the log are unaffected. |
| 22 | **DST transition hits a recurring occurrence** | Nonexistent local times shift forward to the first valid instant; ambiguous ones take the first. The shift is recorded on the expanded occurrence and marked in the calendar view rather than applied silently (§3.2). |
| 23 | **Recurrence horizon exhausted** (device asleep a year, expansion window runs out) | The horizon is a projection property, extended in the background and on view access. Worst case is a brief expansion on first open, never missing or wrong data — the rule is the truth, not the expansion. |
| 24 | **Unbounded series with no `UNTIL`/`COUNT`** | Legal and expected ("get up at 7" has no end date). Bounded only by the rolling horizon, so it costs one row per occurrence in the window and nothing in the log. |
| 25 | **Recurring instance missed** | Recorded as lapsed. Does not roll over, stack, or generate a backlog, by explicit design (§3.6). Feeds the trailing completion rate; a persistently lapsing series is surfaced as a cue problem, not a character problem. |
| 26a | **Stale replica reconnects holding redacted content** | The retained redaction destroys it on ingest. Stickiness is what prevents anti-entropy from silently undoing every redaction (§1.1). |
| 26b | **Peer unreachable when a redact is issued** | Queued and retried with backoff across restarts; the peer is shown as outstanding in the UI, so incomplete propagation is visible rather than assumed. |
| 26c | **Device never returns after a redact** | It keeps its copy. Unfixable in any peer-to-peer design; stated rather than papered over. |
| 26d | **Edit races a redact on the same note** | Redact wins unconditionally. The edit is retained as an event but its body is destroyed like any other. |
| 26h | **One device issues a mass redact** (bug or compromise) | Bounded: 64 targets per redact, and destruction is queued for the cancellation window, during which any device can `redact_cancel`. Content is hidden immediately, so the delay costs nothing against the real threat while making a runaway loop recoverable (§1.1). |
| 26i | **Restore replays events that were later redacted** | They are destroyed on arrival — correct for ordinary recovery. Undoing a regretted redact is a separate, explicitly-flagged operation with a stated cutoff (§1.2), not a silent default. |
| 26e | **Bad events replicated everywhere by a buggy client** | Not a sync failure — the protocol is working as designed. Recovery is restore-from-export (§1.2), which is why replication alone is not treated as a backup. |
| 26f | **All devices lost at once** | Export directory plus whatever external tool is pointed at it. Out of the tool's scope by choice. |
| 26g | **Restore path rots from disuse** | Restore replays events through the ordinary ingest path — the same code sync uses — so it is exercised continuously rather than only on the day it is needed. |
| 26 | **External calendar unreachable or removed** | Shadow-namespace entries go stale and are marked as such. Notes captured against those meetings are in the log and survive intact with their ambient context (§3.9). |

---

## 8. Implementation

Rust for everything that is not a platform UI. One cargo workspace, one core, thin shells.

### 8.1 Where the line sits

**Everything that can be wrong lives in the core**, in Rust, compiled once and shipped
everywhere: the event log, signing and verification, the fold, the recognizer, recurrence
expansion, timezone handling, the sync protocol, conflict resolution, the scheduler.

**The shells do three things only**: draw pixels, own the platform hotkey or tile, and read
platform context for the ambient envelope. No shell contains a rule. If a shell has to know
that `occ_cancel` loses to `occ_move`, the line is in the wrong place — that logic must exist
once, or the three platforms will drift and the drift will show up as your notes disagreeing
with each other.

### 8.2 Workspace

```
brain/
  crates/
    capture-core/       # events, log, fold, conflict rules — no I/O, no async
    capture-store/      # SQLite: events.db + state.db, projections, FTS
    capture-time/       # instants vs floating dates, RRULE expansion, DST policy
    capture-parse/      # sigils + grammar recognizer, confidence scoring
    capture-sync/       # BSP: Noise_IK, discovery, anti-entropy, gossip, blobs
    capture-engine/   # composition: enrichment queue, intent cues, scheduler
    capture-ffi/      # uniffi bindings surface for Android
  apps/
    capture-cli/        # `capture` — one binary: capture, read, write, ops, test harness (§8.6)
    capture-overlay/    # resident overlay, egui. The low-latency binary.
    capture-desktop/    # main app: timeline, note page, calendar, browse, review
  android/            # Kotlin + Compose, links libcapture.so
```

`capture-core` is deliberately I/O-free and async-free — pure data in, events out. That is what
makes the conflict rules in §6 testable exhaustively rather than by hand: the interesting cases
are all "given these events in this order, what does the fold say," which needs no runtime at
all. Property tests can assert convergence directly — shuffle any event set, fold it, and the
result must be identical.

That property is only meaningful if **nothing local leaks into the fold**. Two things violated it
in earlier drafts and are now excluded by construction: the wall clock (fold-time quarantine of
future-dated events, §7 mode 4) and the current roster (rendezvous ranking evaluated at fold time
rather than frozen at the capture's HLC, §2.5.3). Both would have made the same log produce
different states on different machines, or on the same machine on different days — and the
property test would have caught neither, since both were specified as intended behaviour. Worth
stating as a rule: **the fold takes the event set and nothing else.**

### 8.3 Dependencies and why

| Concern | Choice | Reason |
|---|---|---|
| Storage | `rusqlite`, bundled SQLite, FTS5 on | Bundled means the same SQLite version on desktop and Android, not whatever the OS shipped. FTS5 is the search view. |
| Canonical encoding | **`minicbor`**, explicit field indices | Event ids are content hashes, so encoding must be byte-identical across platforms or ids diverge and dedup breaks. A correctness requirement, not a preference — see below. |
| Hashing | `blake3` | Fast, and already the id scheme in §1. |
| Signatures | `ed25519-dalek` | Per-event signing (§5.3). |
| Transport crypto | `snow` | Noise_IK for peers, `NNpsk0` for pairing. Both are stock patterns; none of the state machine, nonce handling or rekeying is ours to get subtly wrong. |
| Discovery | `mdns-sd` | Pure Rust, no platform daemon dependency. |
| Time | `jiff` | The tz/DST semantics in §3.2 are the hard part of this whole domain. Jiff models zoned vs civil time as distinct types, which makes the instants-versus-floating-dates split a type error rather than a code review. |
| Recurrence | `rrule` | RFC 5545 expansion; wrap rather than expose, so the crate stays swappable. |
| Async | `tokio` | Sync layer only. Core stays sync. |
| Android bindings | `uniffi` + `cargo-ndk` | Generates Kotlin from the Rust interface. Hand-written JNI for a surface this size would be a permanent tax. |

Pin versions at scaffold time and check current releases then — several of these move quickly.

**Why `minicbor` specifically**, since the encoding is the one decision that can never be revised:

- **Explicit numeric field indices** (`#[n(0)]`) put the wire layout in the type rather than
  deriving it from Rust field names or declaration order. Renaming a field cannot change an id.
- **`no_std` first**, which composes with the purity enforcement in §8.2 rather than fighting it.
- **Deterministic by construction**, not by a configuration flag someone can forget.

The serde-based alternatives were rejected for the same underlying reason. `ciborium` does not sort
map keys and is deterministic only by accident — one `HashMap` or one `#[serde(flatten)]` silently
forks every id derived from that body. `serde_cbor` has been unmaintained since 2021, which is
disqualifying for a format meant to be readable in a decade. `cbor2` does offer canonical encoding
with serde and `no_std` and is the closest runner-up; it loses only because serde's ergonomics are
precisely what let an encoding drift without anyone noticing, and that is the failure this decision
cannot recover from.

**One constraint dissolved on inspection.** An earlier draft treated "unknown fields are preserved
verbatim" (§1.3 rule 1) as a codec requirement, which argued for string-keyed serde maps. It is not:
events are immutable and stored as the exact bytes `bhash` commits to, so reading never re-encodes
and preservation is a storage property that is already satisfied. The codec only needs to *skip*
unknown fields on decode.

**Decoding rejects non-canonical input.** An event whose encoding is not the canonical form is
refused rather than re-canonicalised. Accepting it would only defer the failure: `bhash` is computed
over canonical bytes, so a re-canonicalised body would fail its integrity check anyway, further from
the cause. Rejecting also means no second valid encoding of the same logical event can exist.

**The golden vectors are what actually enforce the freeze.** The crate choice makes drift unlikely;
a committed fixture of logical event → exact bytes → exact id, checked in CI on every target, makes
it *detectable*. Neither substitutes for the other.

**Nonce placement follows from the encoding.** With indexed fields you do not control physical
position, so "the body's first field is a 16-byte nonce" is not a statement you can make. It is
**field index 0**, and sorting first is a consequence rather than a requirement.

### 8.4 UI, per platform

**Capture overlay — `egui`.** Its own small binary, resident, pre-warmed window. An
immediate-mode native window that is *shown* rather than constructed hits the §2.1 latency target
reliably, and the overlay is one text field, so immediate mode costs nothing in expressiveness.

Note the honest weight of this argument: §2.1 records that the 150 ms figure is a UI convention
rather than an evidence-derived requirement, so it should not by itself justify a second UI
stack. It does not have to. A resident single-purpose capture binary is worth having regardless —
it keeps the one path that must never fail independent of the main application's crashes,
updates, and memory footprint. That is the load-bearing reason; latency is a bonus.

**Desktop app — Tauri, and it is the first-party surface.** Calendar grids, timelines and browse
views are exactly the work a webview is good at and immediate-mode GUI is painful for. Startup
latency does not matter here because this is the deliberate door (§3.1.4), not the capture path.
Core is linked directly, not spoken to over HTTP.

**Everything is reachable from it** — reading, editing, tagging, aliasing, pinning, muting,
resolving divergences, and redacting. The CLI is not a lesser twin of it; the two have different
jobs. The CLI's primary roles are **capture from a terminal** (§2.2, where it is the better surface
because it is the only one that can collect context) and **being the test harness** (§8.6). Day to
day, management happens in the GUI.

This makes M1 larger than a read-only GUI would, and that is accepted deliberately: a system you can
read but not act on pushes you to a terminal for ordinary gestures like muting a note you are
looking at, and §2.6 exists precisely so that suppressing something is *cheaper* than destroying it.
A gradient where the low-cost option is terminal-only and the destructive one is a documented
command points the wrong way.

**Redaction is reachable, and its confirmation must be harder to dismiss than a terminal prompt.**
§1.1 warns that the only destructive confirmation in the system must not be trained away by
appearing routinely — and a modal dialog is far more trainable than a terminal prompt, because
dismissing dialogs is a reflex. So the GUI confirmation names exactly what will be destroyed, item
by item, and requires a deliberate act rather than a default-focused **OK** button.

**The pending-redaction countdown lives here**, with a working cancel (§4.5). It is the only live
element in the interface, and a safety net that requires finding a terminal during the window is not
much of a net.

Two UI stacks is a real cost, and worth it: the two surfaces have opposite requirements. One
must open faster than you can think; the other must render a month grid. Forcing either to do
the other's job compromises the one that actually determines whether you use the system.

**Android — Kotlin + Jetpack Compose**, over the same core through uniffi. Compose because the
platform integration surface is large and unavoidable: Quick Settings tile, share-sheet target,
lock-screen widget, assistant intent, geofences, notifications. Fighting that from Rust would
be all cost and no benefit, and it is the one place where "use whatever works" clearly means
the native toolkit.

### 8.5 Platform risks worth knowing before starting

Not blockers, but each is a genuine "spent a weekend on it" item rather than a line of code.

- **Wayland global hotkeys — checked against the target machine, and the intended path is not
  available there.** X11 grabs are trivial; Wayland has no general mechanism by design. The plan
  was the `org.freedesktop.portal.GlobalShortcuts` XDG portal — but on the actual target (Ubuntu,
  GNOME Shell 46, Wayland, `xdg-desktop-portal` 1.18.4) that interface **is not exposed at all**;
  the GNOME portal backend is not installed. KDE has had it far longer than GNOME.

  **Xwayland does not rescue it.** An `XGrabKey` under Xwayland fires only while an X11 window has
  focus, so the hotkey silently dies in every Wayland-native window — nearly everything on GNOME
  46. That is exactly the registered-but-never-fires failure §8.9 exists to detect, so building on
  it would mean knowingly shipping the bug.

  **Resolution: a platform trait with runtime detection**, tried in this order:

  1. **`GlobalShortcuts` portal**, where the interface is actually present — KDE now, GNOME later.
     Implemented in M1 as a detected path so the better mechanism is used wherever it exists.
  2. **GNOME custom keybinding via `gsettings`.** The installer registers a shortcut that runs a
     thin client, which signals the resident overlay and exits. This is what works on the target
     machine today: no extension, no portal, inspectable and revocable from GNOME Settings. It
     costs one process spawn per capture — a few milliseconds against an already-warm overlay.
  3. **`XGrabKey`**, under a genuine X11 session only.
  4. **None** — say so loudly, hold it as a tier-1 condition, and point at the terminal keybinding
     (§8.6), which needs no platform integration whatsoever.

  A GNOME Shell extension would give a true in-process grab and is rejected: separate install,
  breaks across GNOME major versions, and that is the wrong cost for one keybinding.

  Because option 2 is spawn-based, there are more ways for it to be registered-but-not-firing —
  which makes §8.9's "press it once to confirm" load-bearing rather than ceremonial.
- **macOS hotkeys** need Accessibility permission, which means a first-run prompt and a
  code-signed binary to be granted it persistently.
- **mDNS on Android** requires holding a `WifiManager.MulticastLock`. Without it, multicast
  receive silently does nothing — discovery appears broken with no error. Also, Android doze
  will suspend the sync service; treat the phone as intermittently reachable by design rather
  than fighting it, which the protocol already assumes (§7, failure mode 1).
- **Android background execution** limits mean enrichment does not run there. Fine — the phone
  captures, the desktop enriches (§9).

### 8.6 Command line and test harness

One binary, `capture`, doing double duty: the fastest capture path when a terminal is already
focused, and the harness that makes everything else testable.

#### Capture without stealing words

`capture budget 40k revisit Q2` cannot mean capture, because then `capture search foo` is ambiguous
and some captures become unreachable — with no way to know which words were stolen. So capture is
explicitly marked with `+`, which cannot collide with any subcommand name, now or later.

A bare `brain <words>` matching no subcommand **errors and suggests the `+` form**. It never
silently captures: a mistyped subcommand becoming a note is the kind of small betrayal that
erodes trust in the tool (§3.1.3).

#### The shell will eat your capture, and a naive alias makes it worse

**Every sigil in §3.1.1 is a shell metacharacter.** This is the single worst defect an adversarial
review found in the CLI, and it is silent:

| Typed | What the process actually receives |
|---|---|
| `b fix auth #client-acme` | `#` starts a comment — **everything after it is discarded** |
| `b revisit >2026-08-24` | `>` redirects; a file named `2026-08-24` is created |
| `b ship it !p1` | history expansion — error or silent substitution |
| `b review ^project` | history substitution in bash/csh |
| `b needs ~30m` | tilde expansion |
| `b land this *before* the demo` | glob — the capture depends on your current directory |
| `b ask about $AWS_SECRET` | the secret is interpolated into your log |
| ``b retry with `date` `` | **command substitution executes** |

Silent, partial, unrecoverable alteration of input, in the one path §0.2 rates best-supported and
§1's event table calls "**never** rejected, never validated". It would also force a decision at
capture time — *does my thought contain shell metacharacters?* — which is exactly the
working-memory tax §3.1.1 rejects a DSL to avoid.

**A shell function cannot fix this**, and an earlier draft of this section proposed one:

```sh
b() { printf '%s\n' "$READLINE_LINE" | capture + --stdin; }   # DOES NOT WORK
```

That is wrong twice over. `READLINE_LINE` is bash-only *and* is set only inside a function invoked
by a `bind -x` keybinding. When `b fix auth #client-acme` runs as an ordinary command, the shell has
already split, expanded and truncated the line before `b` is entered, and `READLINE_LINE` is unset —
so it pipes an empty line and **captures nothing at all**, which is worse than the truncation it was
meant to prevent. zsh has no such variable.

The general principle: by the time any shell function runs, word-splitting and expansion have
already happened. The raw line has to be intercepted *before* the shell parses it, which means a
keybinding rather than a command.

**Two mechanisms, and the primary one is a keybinding.**

```sh
# bash — reads the raw buffer, then clears it
_capture_overlay() { printf '%s\n' "$READLINE_LINE" | capture + --stdin; READLINE_LINE=''; READLINE_POINT=0; }
bind -x '"\C-b": _capture_overlay'

# zsh — ZLE widget over $BUFFER
_capture_overlay() { print -r -- "$BUFFER" | capture + --stdin; BUFFER=''; }
zle -N _capture_overlay; bindkey '^B' _capture_overlay
```

So the terminal gesture is **type the text, press Ctrl-B** — not "type `b` then the text". That is
a different gesture from the one an earlier draft described, and it is the one that actually works.
It is also closer to the overlay: a chord, not a command.

**Fallback where a keybinding is unavailable** — a non-interactive shell, an unusual shell, a
restricted environment: `capture +` with no arguments reads a line with its own in-process editor,
which involves no shell parsing at all. Slower by one keystroke, correct everywhere.

**Quoting is never required of the user.** Requiring it is the same failure in a politer form —
they would still have to notice, at the moment they can least afford to.

Argument-form capture remains for scripts (`capture + -- <text>`, everything after `--` taken
literally). It is not the human path, and the docs must not present it as one — an implementer who
wires the interactive gesture to this form makes the metacharacter test pass while the human path
stays broken.

#### Platform scope for M1

**M1 targets Linux only**, with GNOME on Wayland as the primary session. Recorded here because it
is a decision rather than an accident of what got built first.

macOS is excluded deliberately. Registering a global hotkey there needs the Accessibility
permission, and that grant is tied to the binary's signature — an unsigned build loses it on every
rebuild and after a reboot. The result is a capture chord that works when it is set up and silently
stops later, which is exactly the dead-gesture failure §8.6 builds detection for, shipped as the
default experience. Supporting macOS properly means signing and notarising, which is a distribution
problem rather than a design one, so it waits until there is something to distribute.

The hotkey mechanism is chosen at runtime behind a trait (§8.6), so adding a platform is additive.

#### stdin, editor, and empty input

Three cases that must not be conflated, since `capture +` with piped input and `capture +` with no
input look identical to an argument parser:

| Invocation | Behaviour |
|---|---|
| `capture + --stdin` | read stdin to EOF, verbatim |
| `capture +` with stdin **not a tty** | read stdin (pipes work without the flag) |
| `capture +` with stdin a tty | open `$EDITOR` |
| `capture + -- <text>` | capture the literal remainder |
| empty result, any path | **capture nothing and say so** — never a blank note |

Capture through the CLI carries the same guarantees as the GUI because it is the same core: never
blocks on network, spools on a full disk. It prints the slug and nothing else.

#### Surface

**Read** — `timeline`, `show <slug>`, `search <query>`, `links <slug>`, `agenda`, `cal`

**Write** — `edit` ($EDITOR), `tag <slug> +foo -bar`, `alias`, `done`, `mute [--until]`,
`pin`, `promote <slug> <type> …` (correct a misparse — §3.1.3 promises this works from any
device), `intent` (create/edit an if-then plan — §2.4), `carries-forward <slug>` (§3.6),
`notifier` (§2.4.1), `resolve <slug>` (dismiss a divergence — §6), `redact`,
`redact --cancel <redact-id>`

Those middle commands exist because the design promises each of those operations works "from any
device", and a device with only a terminal is a device. Without them the `--json` harness cannot
drive the states the property tests care about.

**Ops** — the half that earns the harness label:

| Command | Why it earns its place |
|---|---|
| `capture log <slug>` | The raw events behind a note. Every fold bug reduces to "what events actually produced this row", so this is the highest-value debugging command in the set. |
| `capture verify` | Recomputes every `id`, checks every signature, **checks `blake3(body) == bhash`**, and walks every origin's hash chain. The body check is not optional: `id` and `sig` authenticate the body's *hash*, so without it a relaying device can substitute content and every downstream replica accepts it (§1). Turns §1's integrity claims into something runnable, including that redacted rows still verify (§1.1). |
| `capture fold --rebuild` | Forced re-fold, timed. Exercises the §1.3 migration story on demand rather than only during an upgrade. |
| `capture status` | §4.5's status page as text |
| `capture quarantine [--reason]` | What did not fold, and why |
| `capture peers` / `pair` / `sync --now` | M3 |
| `capture export` / `restore [--ignore-redactions-after T]` | §1.2, including the deliberate undo path |

**`--json` on every read command.** This is what makes it a harness rather than a convenience:
integration tests assert against structured output instead of parsing tables. Human-readable by
default, JSON on request, meaningful exit codes throughout.

`redact` is the only command that confirms.

**`--force` skips the prompt, not the safeguards.** §1.1 calls the confirmation the only
destructive gate in the system and warns it must not be trained away — and a flag that removes it
is how `while read s; do capture redact --force "$s"; done` becomes the "one loop in one buggy
build" §1.1 names as the threat. So `--force` is explicitly defined as: the 64-target cap holds,
the receiver-side rate limit holds (256 targets/hour/origin, then held for confirmation), the
cancellation window holds, and the pre-redact snapshot is still written. It suppresses a prompt and
nothing else. `capture redact --cancel` accepts a redact event id, and `--cancel --all-since <t>`
exists precisely because cancelling a runaway loop by hand would otherwise be impossible inside
the window.

**Exit codes.** `0` success; `1` usage; `2` not found; `3` integrity failure. `capture verify`
returns `0` when every *authentic* event verifies — the presence of rows in the `rejected` table
(§5.3) is expected operation on a live network, is reported in the summary, and is not itself a
failure. It returns `3` only when an event that is stored as authentic fails to verify, which is
the condition that actually means something is wrong.

### 8.7 Due work — who runs the clock

Several behaviours are time-driven: redaction destruction one window after the redaction's own HLC,
snapshot expiry at thirty days, dead-gesture detection after an idle period. M1 has no daemon — a
resident overlay, an intermittently-running desktop app, and a transient CLI — and an earlier draft
left the question of who owns them unanswered, which would have meant "destroyed whenever you
happen to open something."

**A due-work queue in the projection, drained from two triggers:**

- **Every process checks it on startup.** This is the floor and costs nothing.
- **The resident overlay additionally runs a low-frequency timer**, since it is already resident for
  the capture gesture and is the one process with a legitimate claim to being up.

If the overlay is not running, the system falls back to startup-only. That degrades *latency*, not
correctness — nothing is lost, it just happens later.

Deliberately not a separate daemon: a fourth binary means service files per platform, a supervision
story, and one more thing that can be silently down. Deliberately not a systemd timer either — it
would move scheduling outside the application, where `capture status` cannot see it, and the whole
point of the health surface is that the system reports its own state honestly.

**Consequence that must be surfaced:** destruction latency now depends on the overlay running, so
`capture status` reports **when due work was last processed**. Without that, a long-dead overlay means
redactions sitting undestroyed while the interface reports them complete — which is precisely the
failure that put the redaction queue in the rebuildable projection rather than in local state
(§1.4).

**Spool drain is not on this queue.** It is event-driven, not time-driven: retrying on a timer while
the disk is still full is pure waste, and a disk recovering is invisible to a clock. It drains on
the next successful append and on startup.

**Export is manual.** `capture export`, run by you, or wired to cron or a systemd timer if you want
it. Scheduling it inside the application would invite the assumption that backups are handled —
and §1.2 is explicit that offsite copying is out of scope and that replication is not backup. A
backup you believe in and do not have is worse than one you know you lack.

### 8.8 One store, several processes

Three binaries open the same store: the resident overlay, the desktop app, and the CLI. WAL handles
many readers plus one writer, so locking is largely solved — but three things are not, and an
earlier draft left all of them unstated.

**A single writer, elected by an advisory lock.** A process takes a lock on the store to become the
writer; the overlay normally holds it, and if the overlay is not running the first CLI invocation
takes it for its lifetime. Everyone else opens read-only and submits writes over the overlay socket.

This matters because WAL prevents two writers from interleaving *transactions* — it does not prevent
two processes from independently deciding to fold the same events, which is a correctness problem
rather than a locking one. One elected writer gives a single owner for the fold, for rebuilds, and
for the due-work queue (§8.7): the same problem three times, answered once.

**Reads go direct, always.** Read commands and `capture verify` open the store read-only without
involving the writer. Diagnostics that stop working when the system is unhealthy are diagnostics you
cannot use at the moment you need them, and `capture verify` in particular exists for exactly that
moment.

**`synchronous=FULL` is a connection-open invariant, not a property of the capture path.**
`journal_mode=WAL` persists in the database file; `synchronous` does not — it is per connection. If
only one binary sets it, then "a capture the interface accepted is never a lie" holds for that
binary and quietly does not for the others.

**Rebuild swaps by generation, never by rename.** Renaming a new file over an open SQLite database
leaves other processes reading the old inode, and the `-wal` and `-shm` sidecars are named from the
*path* — so the new file can end up paired with the old file's write-ahead log. Instead: a
`generation` counter in `meta`, the rebuilder writes the next generation alongside, bumps the
counter, and readers notice on their next transaction and reopen. Nothing is renamed over anything.

**Readers learn of new events by polling `heads`.** A single integer per origin, checked on a short
interval, costing nothing and requiring no coordination with the writer. Push notification over the
socket is a later optimisation and is not needed for correctness.

*Rejected: making the CLI a pure client that never opens SQLite.* It eliminates the whole class of
problem and is genuinely tempting — but it means capture stops working whenever the overlay cannot
start, which breaks the one rule §0.2 rates highest at precisely the wrong moment.

### 8.9 First run

#### Startup states — three, and confusing two of them corrupts the log

```
no ~/capture                                     → fresh
identity + events.db present, agreeing         → normal
events.db present, identity missing/mismatched → RESTORE  (mint a new incarnation)
identity present, events.db missing or empty   → RESTORE  (mint a new incarnation)
```

An earlier draft enumerated only the first three, and the missing case is the more dangerous
direction. "Identity survived, events lost" is reachable two ways — a crash between creating the
keypair and writing the first event, and a user deleting `events.db` to "start clean" — and in
both the identity file *agrees with itself*, so nothing trips. The device then re-allocates seq 1
onward at positions peers already hold: the exact silent permanent loss §1 exists to prevent, in
mirror image. §1 says the incarnation is minted on "restore, reinstall, or database replacement",
and database replacement is precisely what the old check could not observe.

Detection is therefore two-sided: the identity file records the current origin and its highest
written seq, and the two are compared on every startup.

**The comparison is directional, and an earlier version of this paragraph had it wrong.** It said
*either* half being inconsistent means a new incarnation. That is symmetric, and symmetric is
wrong, because the two records are written in a fixed order: the event commits first, the
watermark after. A crash between them therefore leaves the watermark **behind** the log — which is
the ordinary post-crash state, not a restore. Minting an incarnation for it would start a fresh
origin after *every* crash, abandoning the previous origin's sequence each time and filling the
roster with dead origins.

So:

| disagreement | what happened | what to do |
|---|---|---|
| watermark **behind** the log | crash between committing and recording | update it, carry on |
| watermark **ahead** of the log | the store lost events it had already numbered | mint a new origin |

Only the second is a restore, and it is the one that is otherwise invisible: deleting `events.db`
to "start clean" produces it, and the identity file agrees with itself throughout.

**The stop blocks appends, never capture.** An earlier draft made this a hard stop on startup,
which contradicts the system's defining property — §7 modes 1 and 2 make "local append always
succeeds" the rule, and §8.9 argues four paragraphs later that blocking first run over a hotkey
"turns a degraded install into no install". Captures made while identity is unresolved go to the
`spool` (§7 mode 6), which already exists for exactly this shape of problem: input accepted now,
sequenced later. Nothing is lost, and nothing is written to a position that may not be ours.

#### Fresh install

1. Create `~/capture`, generate the static keypair, set incarnation 1.
2. Preallocate the 10 MB spool (§7 mode 6) — done now, while there is definitely disk, because
   its entire purpose is working when there is not.
3. Emit a **self-signed `device_add`** for itself, establishing the roster root.

   This *is* a special case, and an earlier draft claimed otherwise. §5.3 requires membership
   events to be signed by an existing member, and the first device is not yet a member — so the
   rule has to be stated precisely rather than waved at: **a self-signed `device_add` is valid
   if and only if it is `seq` 1 of an origin with no predecessor and the log contains no other
   roster root.** That is narrow, checkable, and bounded. The alternative — "an empty roster
   means trust whatever is local" — is the version that becomes a hole later, because it has no
   such bound.

   Restore does not use this path. A restored device mints a new origin under its **existing
   key**, which the roster already contains (§5.3, membership is key-keyed), so it emits an
   origin claim verifiable by every peer rather than a fresh roster root.
4. Name the device from the hostname, editable. The display name is cosmetic: origin identity is
   `H(pubkey ‖ incarnation)`, so renaming never touches sync.
5. Register the global hotkey — see below.

No account, no cloud step, no pairing prompt. A single device is a complete, working system;
pairing is offered when there is a second device, not before.

#### Defaults are compiled in, not written to the log

`setting` events are **overrides only**. First run writes none.

The alternative — materialising every default as an event — makes those choices permanent. Ship
one bad default in v1 and every install that ever ran v1 carries it forever, because the log
records it as a decision the user made rather than a value the software chose. It also destroys
the ability to answer "was this deliberate, or just the default?", which is the question you
actually want answered when something behaves oddly a year later.

Cost: `capture status` distinguishes *default* from *set by you*. One column, worth it.

#### The empty state

A blank timeline and one line naming the capture gesture, repeated until notes exist.

There is deliberately nothing else to onboard. §3.1.1 commits to a recognizer rather than a DSL,
so there is no syntax to learn; sigils are discovered by seeing them in review (§3.1.1), which
requires having notes first. Sample or seeded notes are excluded outright — fake entries in a
system meant to hold your actual thinking is the wrong first impression, and deleting them is
work you did not ask for.

The empty timeline is the *only* place the hotkey can be discovered, so it states it plainly and
keeps stating it.

#### Hotkey registration — the most likely way M1 fails for non-design reasons

Per §8.5 this is genuinely fragile: Wayland needs the `GlobalShortcuts` portal, which is recent
and unevenly implemented across compositors; macOS needs Accessibility permission granted to a
signed binary.

**Registration succeeding is not the same as the hotkey working**, and the difference is the
realistic failure. On X11 a grab is exclusive and a later grabber silently takes it; on macOS a
system-reserved chord is consumed before your handler ever sees it; on Wayland a compositor may
accept a portal binding and route the key elsewhere. All three produce a green install, zero
events, and §8.9's own diagnosis: indistinguishable from "I never used it", and attributed to the
concept failing.

So registration is **confirmed, not assumed**:

- First run asks you to **press the chord once** and does not consider setup complete until the
  event arrives.
- The default chord is named explicitly so conflicts can be reasoned about at all: **`Ctrl+Alt+Space`**
  on Linux and Windows, **`⌥Space`** on macOS. Both are checked against known system bindings at
  registration and a conflict is reported with the conflicting owner named where the platform
  exposes it.
- **Dead-key detection**: a hotkey registered but never fired for 7 days, on a device with other
  activity, becomes a tier-1 health item. That is the only way this failure is ever noticed.

**If registration fails, it fails loudly and the app continues with the CLI as the capture path.**
Not silently, and not by blocking first run. In that state the empty timeline **swaps its message
to the CLI instruction** — otherwise a GUI with no `+` button (§4.2) and a dead hotkey has zero
capture affordances while advertising a gesture that does nothing.

- Blocking would be worse: `capture +` is a genuinely usable capture surface, and refusing to start
  over a hotkey turns a degraded install into no install.
- Silence would be much worse. "I installed it and never used it" and "I installed it and the
  hotkey never registered" are indistinguishable from the inside — and the second one would get
  attributed to the concept failing, which is precisely the conclusion §8.10's observation window
  is supposed to test honestly.

So it stays a **tier-1 health item** (§4.5) until resolved, with the platform-specific fix named
rather than a generic error.

### 8.10 Build order

Deliberately not sync-first, even though sync is the most interesting part to build.

**M1 — one device, no sync.** `capture-core` + `capture-store` + `capture-overlay` + `capture-cli` +
`capture-desktop`. Capture, store, fold, search. Use it daily on the desktop.

M1 ships a **GUI**, and it is the first-party surface (§8.4) — the reading surface is half of
whether this is pleasant to live with, and a terminal-only trial would risk concluding the concept
fails when the truth was that the reading surface was thin.

Scope of the M1 GUI: **timeline, note page, search, and every mutation** — edit, tag, alias, pin,
mute, resolve, redact — plus the pending-redaction countdown with a working cancel. Not in M1: the
calendar, the agenda, review, and anything that depends on features later milestones bring.

That is larger than a read-only GUI, and the cost is accepted: management that requires a terminal
would push ordinary gestures — muting the note you are looking at — into a second tool, and §2.6
exists precisely so suppressing something is cheaper than destroying it.

The CLI is not thereby demoted; it has two jobs the GUI cannot do. It is the **better capture
surface from a terminal**, because it is the only one that can collect context (§2.2), and it is
the **test harness** (§8.6).

`capture verify` is in M1 rather than deferred to M3. It is cheap, and it catches encoding and id
mistakes at the point where they are still fixable rather than after a year of events have been
written under a wrong rule — which is precisely the class of bug that adversarial review found in
this document.

This is the milestone that puts the design in contact with reality, and it should be reached
fast. But the earlier draft attached a **three-week kill criterion** to it, and that was wrong
twice over — badly enough to be worth recording rather than quietly deleting.

**It contradicted the evidence it claimed to follow.** Lally's median is 66 days, range 18–254;
the 2024 review reports means of 106–154. Three weeks is 3–7× short of the median and below the
fastest individual in Lally's sample. Nothing in that literature predicts "sticks quickly or not
at all" — it predicts a long, noisy, highly variable curve. A three-week verdict would fire
during the period the evidence says is *normal* for a habit that is forming perfectly well.

**It manufactured exactly the failure signal §3.6 forbids.** Two sections of this document argue
that all-or-nothing verdicts should not be constructed, and then this one constructed the largest
one in it — a pre-committed judgement, on a wrong timescale, about whether the thing works,
delivered to the person who built it.

Replaced with:

- **No kill criterion.** M1 is used because it is useful, or it is not. That signal does not
  require a deadline attached to it.
- **A 90-day observation window** before drawing any conclusion, matching the actual distribution
  rather than a convenient sprint length.
- **What is measured is cost, not just counts** — see §0.1 finding 6. An anxious user maintains
  performance at rising effort, so "captures are happening" can read green while the thing gets
  steadily more expensive. Capture rate alone would miss precisely that.

**A related tension this plan has and cannot fully resolve:** context stability is among the
strongest accelerators of habit formation, and a five-milestone build changes the capture
surfaces repeatedly across exactly the 66–154 day window when stability matters most. The
mitigation is to freeze the M1 capture gesture — the chord, the overlay, the commit key — and
treat it as fixed for the duration, changing everything else around it. Whether that is enough is
not knowable in advance, but building the capture surface *last* would be worse.

**M2 — types, and the evidence-backed components.** `capture-time` + `capture-parse`. Promotion,
tasks, events, recurrence, calendar view — **plus scheduled review (§2.8), intents firing locally
(§2.4), and activation (§3.9).** Still one device.

This is the deliberate change, and it is the answer to §9 Q0. An earlier build order shipped every
evidence-backed component in **M5, last** — after the sync protocol, after Android. A second
adversarial review pointed out what that guarantees: the design would have *stated* the risk that
protocol work displaces the components with evidence, and then *built* that risk. Worse, M1 was
defended as the cheap way to answer "does this help", which it cannot do — M1 answers "am I
capturing." Whether any evidence-backed component helps could not be answered at all until the
end.

None of the three needs sync, Android, or enrichment. Review needs a scheduler and a list. Intents
need a clock and a notification. Activation needs the recurrence machinery M2 already builds plus
one event kind. They are in M2 because they are cheap there and because putting them last was a
choice that made the project's stated worst case structurally certain.

**M3 — sync.** `capture-sync`. Second device, pairing, anti-entropy, live push. Conflict rules
exercised by property tests long before this point.

**M4 — Android.** uniffi bindings, Compose shell, capture surfaces, geofence cues. Location cues
and the phone-as-notifier arrive here; until then intents fire on the desktop, which is where
`app_focus` and `git_branch` cues belong anyway (§2.4.1).

**M5 — enrichment.** Transcription, entities, links, the specificity pass. This is genuinely last:
it is the most expensive to build, it is the part with the *least* evidential support, and §2.5
argues it is justified as load reduction rather than as a benefit in itself. Everything works
without it — an un-enriched capture is still a capture.

M1 is small enough to be usable in days, not weeks, and M2 is where the design first becomes
testable against its own premise rather than merely usable.

**What this reordering does not fix.** Roughly 61% of this document is log, sync, conflicts,
failure modes and implementation, none of it supported by anything in `EVIDENCE.md` (§9 Q0). Moving
three components earlier changes the order in which effort lands; it does not change that ratio.
The honest statement is that the durable layer is the price of having any of this on more than one
device, not that the evidence asked for it.

---

## 9. Open questions

Grouped by when each one actually blocks something. Decided items are recorded in place, not
here — language is settled (Rust, §8), as are the parser tiering (§3.1.2), the offline parse
path (failure mode 20c), the storage model (§1), redaction (§1.1), export (§1.2), enrichment
assignment (§2.5.3), notifier selection (§2.4.1), and blob lifetime (§5.6).

### The question above all the others

0. **Does any of this evidence support building this tool?** No — and §0.0 now says so directly.
   Not one cited study tested a knowledge-management system. The literature justifies designing
   around *capture cost*, and nothing more; it cannot tell you whether the resulting tool helps.
   Meanwhile the two best-supported interventions in the same evidence set are **not software**
   (medication, cognitively demanding exercise) and one is a clinical protocol (c-MeST).

   The realistic worst case is not a bad feature. It is that building a five-milestone,
   three-platform, two-UI-stack system with a hand-rolled sync protocol becomes the thing that
   happens *instead* of the interventions with actual evidence behind them.

   **And a second adversarial review quantified how far the design leans that way.** Counting by
   section: the log, sync protocol, conflicts, failure modes and implementation come to roughly
   **61% of this document**, none of it supported by anything in `EVIDENCE.md`. Everything the
   evidence actually reaches — if-then plans, imposed-schedule retrieval practice — is about
   **4%**, and two-thirds of *that* is an admitted child-sample extrapolation. The component with
   the cleanest evidential mapping, scheduled retrieval practice, has **no section at all**: it is
   a table row, an event kind, and a deferred open question.

   Worse, §8.10 ships every evidence-backed component in **M5, last** — after the sync protocol,
   after Android. So the build order structurally guarantees the outcome this question names.
   M1 being small answers "am I capturing", which is worth knowing, but it cannot answer "does
   any evidence-backed component help", because none of them exist until the end.

   **`EVIDENCE.md` §5 also names a large effect this design does not contain at all**:
   behavioural activation, g = 0.85 individual and 0.69 internet-delivered, indistinguishable from
   CBT across 15 RCTs, and straightforwardly software-deliverable. It is the most obvious
   evidence-supported feature nobody has proposed, and its absence from the earlier framing of
   this question — which listed only alternatives a software project *cannot* become — is what
   kept the honest conclusion out of reach: **the evidence says build a somewhat different piece
   of software.**

   **Acted on, not filed.** §8.10 now ships scheduled review (§2.8), intents, and activation
   (§3.9) in **M2** rather than M5, and review — previously a table row and an event kind — has a
   specification. Behavioural activation is in the design as activity scheduling with annotation,
   scoped honestly as scaffolding rather than therapy.

   This is the first *additive* change either review round produced. Every previous correction was
   subtractive — withdraw a claim, add a brake, cap a scope, downgrade a rating — which is how a
   document becomes more careful about what it says while building exactly what it was always
   going to build. Recorded here so the next review can check whether that pattern resumed.

   The question itself stays open. Moving components earlier makes it *answerable* around M2
   instead of never; it does not answer it.

### Blocks M3+ — needed before or during sync

### Tune with use — no decision needed up front

7. **Resurfacing intervals.** The shape is now specified (§2.8) — bounded sessions, selection
   priority, expansion tuned by engagement rather than accuracy. What remains open is the interval
   arithmetic: spacing is well studied for facts (d = 0.54 in classroom settings) and untested for
   personal episodic material, so SM-2 is not adoptable wholesale. Tune against real engagement.

8. **Where the confidence floor sits (§3.1.2).** Too low and the calendar fills with things you
   never scheduled; too high and promotion never fires. Empirical. The suggestion-queue length
   in review is the signal to tune against.

9. **Timezone travel semantics.** You fly to Tokyo. Does "gym at 07:00" fire at 07:00 Tokyo or
   stay pinned to Denver? Genuinely ambiguous and it differs per series — a wake-up alarm
   should follow you, a standing call with someone in Denver must not. Probably a per-series
   flag with a sensible default, but the default matters and should be chosen from use rather
   than guessed now.

10. **All-day and timed events in one grid (§3.2).** The separate band is right for day and
    week. Whether month view can honestly show both without reintroducing the off-by-one-day
    ambiguity is less clear.

11. **Retention of lapsed recurring instances.** Kept because the trailing rate needs them, but
    a decade of "did not walk the dog" rows carries real weight if it ever becomes browsable.
    Probably aggregate after a year — keep the rate, drop the rows. Decide deliberately rather
    than defaulting to keeping everything.

11a. **Whether the trailing completion rate is actually kinder than a streak** (§3.6). A streak
    resets to zero and can be rebuilt starting tomorrow; a 60-day rate stays bad for two months
    regardless of what you do today. Both quantify failure. This is a guess about motivational
    dynamics with no evidence behind it either way, and it should be judged by whether it feels
    punishing in use — not defended by citation.

11b. **Visual capture** (§0.4). Skodzik found the adult ADHD deficit is **verbal**, with visual
    long-term memory intact — and this design is overwhelmingly verbal, converting its one
    non-verbal input to text as fast as possible. Screenshot capture, image notes, sketch input
    and spatial browsing would lean on the intact channel instead. Genuinely the most interesting
    unexplored direction here, and it is unexplored only because the design started from text.

### Deferred by choice

12. **Ambient capture** — the bounded, explicitly-armed session module (§2.9). Not scheduled.
    The privacy tradeoff needs a real conversation before any of it is built.

14. **At-rest protection and device loss** — deferred as a cluster, because the pieces only make
    sense together. Nothing in the data currently warrants treating it as secret; the realistic
    adversary is a stranger holding a stolen phone, not a targeted one. Revisit as a unit:

    - **Encryption at rest.** SQLCipher over the whole store (indexes and FTS included, so
      search cannot leak around it), key held in the platform keystore. *Deferring costs
      nothing structurally:* it is a one-time re-encryption of the file, not an architectural
      commitment, so M1 storage does not foreclose it. The one free thing worth doing now is
      simply confirming platform full-disk encryption is actually on — default on Android,
      opt-in on macOS, frequently absent on Linux desktops.
    - **App lock.** Biometric gate on *reading* the archive, never on capture — gating capture
      would put a decision and a delay in the one path that can afford neither.
    - **Expiry wipe.** A remote wipe is close to theatre, because a stolen device is exactly
      the one that will not hear you; the workable inverse is a device that destroys its
      replica after N days without a successful sync. Safe here in a way it is not elsewhere,
      because every device is a full replica — and the version vector proves it: **wipe only
      when this device's authored events are acknowledged by another live replica; otherwise
      lock but do not destroy.** That single rule is what keeps three weeks abroad with only a
      phone from eating the trip's notes.
    - **Panic redact.** Local-only immediate destroy, no waiting for peers, on the theory that
      the device in your hand is the one you are worried about right now.
    - **Backups versus encryption — the sharp interaction.** Export (§1.2) writes plaintext
      events to a directory beside the store. Encrypting `events.db` while leaving plaintext
      exports next to it protects nothing: the export becomes the easiest way in, and it is the
      copy most likely to be synced to a cloud drive or an external disk by whatever tool is
      pointed at it. Whatever at-rest scheme is chosen has to cover the export path in the same
      pass, which is harder than it sounds — the export must stay restorable on a device whose
      keystore is gone, which is precisely the scenario a backup exists for. A key escrowed
      outside the device (printed recovery phrase, hardware token) is the usual answer and is a
      real design problem, not a checkbox.
    - **Pre-redact snapshots** (§1.2) compound this: they are unencrypted copies of content
      explicitly declared as must-not-exist. If at-rest encryption lands, snapshot policy has
      to be revisited at the same time rather than inherited.

    Decided now and not deferred, because it is a default rather than a subsystem:
    **lock-screen notifications show the fact, not the content** — "Brain: 1 reminder",
    expanding only after unlock (§2.4). Point-of-performance delivery otherwise prints your
    reminders onto a locked screen in someone else's hand, which would leak straight through
    every other measure above. Related and equally cheap: OS cloud backup is disabled for the
    app, so the store is never shipped to a vendor by default.

13. **Attendees on events** (§3.3) are mentioned but unmodelled. Only becomes real if external
    calendar import (§3.9) lands, so it can wait.
