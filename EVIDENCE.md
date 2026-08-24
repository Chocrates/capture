# Evidence base, non-software interventions, and the case against

**Purpose.** The citation record `DESIGN.md` needs, the exercise/non-software thread, and —
added after a second and deeper literature pass — **the strongest available arguments against
building this system at all**, collected deliberately so an adversarial review has real material
rather than sourcing gaps to attack.

**Written** 2026-08-20. Second pass same day, roughly doubling the base.

---

## 0. Read this first — sourcing limitation

Everything below came from **web search result summaries, not full-text paper reads**. Titles,
journals, effect sizes and headline findings were surfaced by search; the papers themselves were
not opened. Treat every number as "reported by a search summary" rather than verified.

Before building an argument on any specific effect size, pull the actual paper. Citations are
given precisely enough to find them, deliberately.

Nothing here is clinical advice. The user has said they have ADHD, depression, and anxiety; that
is context for relevance, not a basis for anyone in a session to make treatment recommendations.
Decisions about medication, therapy, or exercise belong with their clinicians.

---

## 1. Core findings

### 1.1 Memory and ADHD

**Skodzik, Holling & Pedersen (2017), *J Atten Disord*** — meta-analysis of long-term memory in
adult ADHD.

- Worse on **verbal** long-term memory. **No** deficit on **visual**.
- The deficit is at **encoding**, not retrieval.

The verbal/visual split is unexploited by the design and is recorded as an open direction
(`DESIGN.md` §0.4, §9.11b).

**Knouse, Rawson, Vaughn & Dunlosky (2016), *Clinical Psychological Science*** — college
students.

- Retrieval practice benefited ADHD students **to the same degree as controls**. A *null
  moderation*, not an ADHD-specific finding.
- **Self-directed** pacing is where ADHD students lose ground; imposed structure keeps the
  benefit equal.
- Related work: **unmedicated** participants worse at encoding and final test, using fewer deep
  strategies; **medicated** participants not significantly different from controls.

### 1.2 Habit formation

**Lally et al. (2010), *Eur J Soc Psychol***: N=96, median **66 days** to automaticity, range
**18–254**. A single missed day cost under half a point and recovered quickly. Context stability
is a strong accelerator.

**Systematic review (2024), *Healthcare* 12(23):2488**: 20 studies, N=2,601, study means
**106–154 days**, individual range 4–335.

### 1.3 Implementation intentions — and the gap, now confirmed explicitly

**Breitwieser et al. (2025), *British Journal of Psychology***: 42 studies, N=12,957,
**g = 0.31** — a small effect. Stronger in **younger children**; this is a **child** meta-analysis.

The second literature pass found the gap stated outright in the adult ADHD clinical literature:
**implementation intentions have "not yet been studied in adults with ADHD."** Non-clinical
studies in children with ADHD indicate better task follow-through, and if-then planning appears
as a component of CBT for adult ADHD (Ramsay's "Turning Intentions Into Actions"), with one small
open study of unmedicated adults showing improvement on an activation measure.

So the design's most heavily-engineered evidence-derived feature (§2.4, an event kind, a cue
taxonomy, a notifier election protocol) rests on **children plus clinical plausibility**. That
is not nothing, but it is not what §0 originally implied.

### 1.4 Depression and autobiographical memory

**OGM**: robust meta-analytic evidence; persists in **remitted** depression (Hallford 2022,
*Clin Psychol Psychother*); predicts course with **small** effects, larger in clinical samples.

**c-MeST RCT (Hallford et al. 2020, *Behav Res Ther*)**: 35.7% vs 60.6% meeting MDE criteria at
one-month follow-up; specificity **d = 0.53–0.93**; symptoms **d = 0.57**. Gains correlated with
reduced rumination, less cognitive avoidance, better problem-solving.

**c-MeST is a dosed, valence-balanced, supervised clinical protocol.** Not a note-app prompt.

### 1.5 Anxiety

**Attentional Control Theory (Eysenck et al. 2007)**; meta-analysis *Clin Psychol Rev* (2018).
Anxiety impairs **efficiency** more than **effectiveness** — worry preempts working memory, and
performance is frequently **maintained at higher cost** (same accuracy, slower).

### 1.6 Reward learning in depression

Reduced reward-prediction-error signal; less reliance on goal-directed reward learning
(*Neuropsychopharmacology*, 2018). Anhedonia decomposes into liking / wanting / **learning**.

Caveat: this does **not** entail that push beats pull. Blunted reward learning equally predicts
that pushed payoffs are less reinforcing, and that unrewarding notifications train avoidance of
the channel.

---

## 2. Evidence bearing directly on tool design (second pass)

This section did not exist before and is where the sharpest material is.

### 2.1 Cognitive offloading — the central objection to a second brain

**Sparrow et al. (2011)**, "Google Effects on Memory": when people expect information to remain
externally available, they remember **the information** less and **where to find it** more.

**Gong & Yang (2024), *Frontiers in Public Health*** — meta-analytical review: the Google effect
is **real but strongly moderated** by perceived reliability of the external store, device type,
and cognitive load. A separate 2025 meta-analysis (PubMed 40500483) on offloading and
memory-based task performance reports **small-to-moderate deficits in working and long-term
memory under habitual external access**. The original Sparrow finding has a **mixed replication
record**.

**Why this matters more than anything else here:** the design's premise is that externalising
storage bypasses an encoding deficit. This literature says externalising storage may *deepen* the
deficit — and the moderator is *perceived reliability of the store*, which a well-built,
always-available, synced-everywhere system maximises. **The better this system works, the
stronger the offloading effect should be.**

The honest counter is that this is a trade the user is entitled to make knowingly: offloading
what you would otherwise simply lose is not the same as offloading what you would otherwise
retain. But the design currently does not acknowledge the trade at all.

### 2.2 The generation effect and desirable difficulties

Generative processing — retrieving, organising, elaborating — produces durable memory.
Learners remember vocabulary roughly **twice as well when writing definitions than copying**.

`DESIGN.md` §2.5 does the organising work *for* you: titles, entities, links, summaries. On this
literature, that removes exactly the effortful processing that produces retention. Automating
enrichment may make the archive better and your memory worse.

**The principled counter exists and is in this literature**: desirable difficulty effects
**fail** when working memory is already stressed by high element interactivity — under those
conditions added difficulty is *undesirable*, not desirable (see the "Undesirable Difficulty
Effects" line of work, PMC6099118). A user with ADHD plus anxiety is close to a worked example of
already-stressed working memory.

So the design's automation is defensible — but as *load reduction under WM stress*, not as
neutral convenience, and the trade-off should be stated rather than assumed.

### 2.3 External memory aids — the design's strongest actual support

Evidence-based compensatory strategy for **prospective memory**, chiefly from acquired brain
injury and MCI rehabilitation. Electronic aids described as "the most comprehensive,
cost-effective means of prospective memory support"; participants completed more everyday
prospective memory tasks when using them. Systematic review and meta-analysis of mnemonic
strategy vs cognitive training vs external aids vs combinations (ASHA evidence map).

Two implementation findings that map onto this design almost exactly:

- **Organisation within the aid was critical** to outcomes — an easy-to-access to-do section, not
  an undifferentiated pile.
- **People needed additional prompts to remember to check the aid at all.** Reminder alarms were
  required alongside the notebook.

That second point is direct empirical support for `DESIGN.md`'s push-not-pull rule (§0.2), which
until now rested on the contestable reward-learning inference (§1.6). **This is a better citation
for that rule than the one currently used.**

Caveat: the population is acquired brain injury and MCI, not ADHD.

### 2.4 Digital interventions for ADHD — the "no study tested a tool" gap, partially closed

**Meta-analytic review of digital mental health interventions for ADHD** (*Internet
Interventions* / ScienceDirect S2451958825001186): 23 RCTs, 99 effect sizes, N=1,472. Small but
significant reductions in overall ADHD symptoms **g = −0.32**, inattentive symptoms **g = −0.25**.

**attexis RCT** (*Psychological Medicine*, 2025): largest RCT to date of a digital intervention
for adult ADHD, CBT-based; effect within the range of face-to-face CBT for adult ADHD
(d = 0.71–0.98).

Also relevant: **behavioural activation** for depression, **g = 0.85** individual (22 RCTs), and
internet-delivered BA **g = 0.69** (11 studies, N=2,060), with no significant difference from
CBT/CT across 15 RCTs.

**What this changes:** `DESIGN.md` §0.0 says "no cited study tested a knowledge-management tool."
That remains true. But "no evidence that software helps ADHD adults" is now **false** — digital
interventions have a real, small, replicated effect. The catch is that every one of them is a
**structured therapeutic programme** (CBT modules, activity scheduling), not a note-taking
system. The evidence supports *software as a delivery vehicle for structured intervention*, not
software as a place to put notes.

That is a meaningful distinction and arguably the most actionable finding in this document: the
components with evidence are the ones this design treats as peripheral (scheduled structure,
activity scheduling, implementation planning), and the component with none is the one it treats
as central (capture and storage).

### 2.5 Notifications and interruption

Receiving a phone notification degrades performance on attention-demanding tasks **even when not
acted on** (Stothart et al.; PMC9671478 for behavioural and electrophysiological evidence).
Task-switching produces **attention residue**. Field experiments show reducing
notification-driven interruption improves performance and reduces strain; a 14-day field
experiment found **batching notifications into three daily deliveries** made users happier and
less stressed, mediated by improved subjective attention quality.

The widely-quoted "23 minutes to refocus" figure circulates heavily in popular sources and should
be treated as soft.

**Bearing on the design:** this supports the notification budget added in §2.6, and argues it
should go further — toward **batched delivery by default** rather than a cap. It also sharpens
the cost of §2.4.1's deliberate "duplicate nudge beats missed nudge" ruling, which was decided on
ADHD grounds with the attention cost unweighed.

### 2.6 Note-taking mechanism — a caution about a common premise

**Mueller & Oppenheimer (2014)**, longhand beats laptop via generative encoding, is widely cited
in note-taking design. It **failed to replicate**: Morehead et al. (2019) and **Urry et al.
(2021)**, a direct replication of Study 1 plus mini meta-analyses (*Psychological Science*), found
no handwriting advantage. Laptop note-takers did take more verbatim notes — but did not perform
worse.

Useful two ways: it removes a potential objection to a typing-first capture design, and it is a
worked example of why this document's citations need checking rather than inheriting.

### 2.7 Working memory training — the cautionary analogue

**Melby-Lervåg, Redick & Hulme (2016), *Perspectives on Psychological Science***: 87 publications,
145 comparisons. Reliable **near** transfer to trained WM tasks; **no convincing evidence of far
transfer** to nonverbal ability, verbal ability, decoding, comprehension, or arithmetic when
compared against treated controls.

Relevant as precedent rather than direct evidence: an entire commercial category was built on a
mechanism that produced training-specific gains and nothing that generalised. Any claim this
design makes about improving memory *itself* — rather than compensating for it — should be held
to that standard.

### 2.8 Expressive writing — undercuts a premise nobody stated but everyone assumes

Pennebaker-style expressive writing has **mixed** evidence for depression. One study found 20
minutes on 3 consecutive days reduced depression at 6 months, mediated by changes in brooding.
But a recent meta-analysis found **no significant long-term effects on depressive symptoms**, and
concluded that **brief, self-directed expressive writing should not be considered an effective
method to decrease depressive symptoms in physically healthy adults**. More sessions and more
specific prompts produce larger effects; benefit is larger for high-brooding individuals.

`DESIGN.md` does not claim writing things down improves mood — but the second-brain genre does,
implicitly and constantly, and any future feature framed as "journaling helps" has weak ground.

### 2.9 Documented harms of mental health apps

**"This app said I had severe depression, and now I don't know what to do": the unintentional
harms of mental health applications** (CHI 2024, ACM). Harms arise when apps surface information
without sufficient support: no guidance on what to do next, poorly considered features that
worsen mood, clinical jargon presented to lay users who then misinterpret it.

Directly relevant to the specificity pass (§2.5.0), scheduled resurfacing, and the trailing
completion rates in §3.6 — all of which surface an assessment of the user to the user.

Mixed evidence on the other side: MoodPrism found engagement with mood self-monitoring predicted
improvement via increased emotional self-awareness.

---

## 3. Comorbidity — the actual population, and it is not the studied one

Nearly every finding above comes from a **single-diagnosis** sample. The user has three.

- **Comorbidity in adults with ADHD is the norm, not the exception**: roughly **85%** have at
  least one comorbid disorder — mood ~60%, anxiety ~30%, substance use ~45%, personality ~35%
  (*Int J Mol Sci* 26(22):11020, 2025; *Frontiers in Psychiatry* 2025;16:1597559).
- ADHD and depression share "cold" cognitive impairments; a meta-analysis across 18 systematic
  reviews attributes the shared deficits to **executive function** — verbal fluency, selective
  attention, working memory, long-term memory — rather than general ability (*Neurosci Biobehav
  Rev*, S0149763420306825).
- **Deficits appear additive.** One study reports additive deficits in executive functioning,
  processing speed, and contextual memory when depression is comorbid with ADHD.
- Mediation work finds ADHD affects quality of life **via** executive function and then **via**
  depressive/anxiety symptoms (*J Affect Disord*, S0165032721006066).

**The finding with the sharpest design implication:** anxiety in ADHD is frequently described as
**secondary to sustained executive strain and repeated performance failures**, rather than driven
by autonomic hyperarousal.

If that is right, then **every failure signal this system generates feeds the anxiety directly**.
That reframes §3.6's rejection of streak counters from a plausibility argument into something
closer to a mechanistic one — and simultaneously indicts the trailing completion rate that
replaced it, plus any "you have not captured in 5 days" nudge, plus §8.8's observation window if
it is ever framed as a verdict on the user rather than on the tool.

---

## 4. Exercise

Sources (2024–2026): *Frontiers in Psychology* 15:1376354; PMC11808027 (network meta-analysis on
exercise type and working memory); *Adolescent Research Review* three-level meta-analysis;
*Italian Journal of Pediatrics* (2025) on vigorous intensity.

- **Cognitive-aerobic** exercise showed the largest effect on working memory; **ball games**
  second.
- **Team sports and coordinative training beat plain aerobic at comparable cardiovascular load** —
  the result that isolates the cognitive component from the fitness component.
- Improvements in core symptoms, executive function, and **subjective** sleep; **no** significant
  effect on objectively measured sleep.

*Limits:* predominantly child and adolescent samples; modest effect sizes; **acute** effects
better established than chronic. Plain aerobic still beat no-exercise everywhere.

### What "cognitively demanding" means

Exercise that loads executive function **while** raising heart rate. Contrast: treadmill,
stationary bike, lap swimming — cardiovascularly hard, cognitively trivial.

**Heuristic: if you could do it while listening to a podcast, it is not cognitively demanding.**

| Open — unpredictable, reactive | Closed — self-paced, predictable |
|---|---|
| Tennis, badminton, table tennis | Running |
| Basketball, soccer | Lap swimming |
| Squash, racquetball | Weightlifting |
| Martial arts sparring | Rowing machine |
| Climbing (route reading under load) | Cycling |

Also qualifying: **coordinative training** (dance, martial arts forms, gymnastics, varied jump
rope — load comes from novel motor learning); **cognitive-aerobic / exergaming**; **skill
acquisition generally**, though this decays as competence grows, which suggests rotating
activities may beat mastering one. That last inference is mine, not the literature's.

### Open questions for a future session

1. **Adult evidence.** Nearly all of the above is child/adolescent. Determines how much the rest
   is worth.
2. **Acute vs chronic.** If largely acute, timing matters more than programme design.
3. **Dose.** Frequency, duration, intensity thresholds — not surfaced at all.
4. **Depression interactions.** Exercise has its own depression literature; the open/closed
   distinction within it is unexplored here.
5. **Adherence** — probably the most important. Open-skill activity needs other people, a venue,
   and a schedule. That practical asymmetry may swamp the effect-size difference entirely.

---

## 5. The non-software interventions — with a correction

Of the interventions here with demonstrated effects on the relevant faculties:

**Medication.** The largest effects in the set — but the second pass **corrects an overstatement**
carried in the first. "Medicated adults were not significantly different from controls" (§1.1)
is one study's result on specific memory measures. Meta-analytically, stimulant effects on
*cognition* in adults are **small**: working memory **g = 0.13**, memory 0.25, vigilance 0.22,
reaction-time variability 0.16 (PMC4968888). Methylphenidate across cognitive domains reaches
**g = 0.34–0.59** (18 studies, N=1,667). Stimulants and atomoxetine show comparable executive
function effects with no significant drug differences (*Neurosci Biobehav Rev*, 2024).

So: medication has strong effects on **symptoms** and small-to-moderate effects on **cognition**.
Saying it is "the largest effect in the evidence set" was true only loosely, and the corrected
version is less dramatic. It remains not-software and remains absent from `DESIGN.md`.

**Cognitively demanding exercise.** Modest, real, mostly child samples. Not software.

**c-MeST.** A real RCT with a clinical outcome. A clinical protocol, not a feature.

**Behavioural activation.** Added in the second pass: **g = 0.85** individual, **g = 0.69**
internet-delivered, indistinguishable from CBT across 15 RCTs. This one *is* deliverable in
software, has a large effect, and is completely absent from the design — the most obvious
evidence-supported feature nobody has proposed.

---

## 6. The case against this design — collected for adversarial review

Strongest arguments against, stated as strongly as the evidence permits.

1. **Offloading may worsen the deficit it compensates for** (§2.1), and the moderator is
   perceived reliability of the store — so the better the system, the stronger the effect. This
   is the deepest objection and the design does not currently acknowledge it.

2. **Automating enrichment removes the generative processing that produces retention** (§2.2).
   The counter — desirable difficulties fail under existing WM load — is real but must be argued,
   not assumed.

3. **The evidence supports structured intervention delivered by software, not note-taking
   software** (§2.4). Digital interventions with effects are CBT modules, activity scheduling,
   implementation planning. The design treats those as peripheral and treats capture — which has
   no supporting evidence — as central. **The components with evidence and the components with
   engineering effort are close to disjoint.**

4. **PKM itself has no empirical base.** A search for rigorous evidence on Zettelkasten and
   personal knowledge management returns practitioner accounts and Luhmann. Luhmann is n=1,
   unblinded, unreplicated, and a textbook survivorship case: the productive sociologist with the
   card index is remembered; the unproductive ones with card indexes are not.

5. **Implementation intentions have not been studied in adults with ADHD** (§1.3) — now confirmed
   explicitly, with the child-sample age gradient running the wrong way.

6. **Every failure signal feeds the anxiety** (§3), if anxiety in ADHD is secondary to repeated
   performance failure. That implicates trailing completion rates, lapse surfacing, the health
   view, and any framing of an observation window as a verdict.

7. **Mental health apps have documented harms** when they surface assessments without support
   (§2.9) — directly applicable to the specificity pass and scheduled resurfacing.

8. **Notifications impose measurable cost even unattended** (§2.5), and the design deliberately
   chose to tolerate duplicates without weighing that cost.

9. **The medication correction** (§5) removes the "just get medicated" straw-man but also removes
   the claim that any single intervention here is decisive.

10. **Working memory training is the precedent** (§2.7) for a plausible mechanism, a commercial
    category, and no far transfer.

**The strongest single sentence against the project:** the interventions in this evidence set
with demonstrated effects are structured therapeutic programmes, medication, and exercise — and a
capture-and-storage system is none of those, while being the thing that will absorb the months.

**The strongest honest defence:** external memory aids for prospective memory *are*
evidence-based (§2.3), including the specific finding that people need prompting to check the
aid — which is what this design does. That is a narrow, real, and defensible claim, and it is a
much smaller claim than "second brain."

---

## 7. Where this connects to the design

`DESIGN.md` §0 was rewritten after the first review. The second pass produced the following, and
a third adversarial review confirmed none of it had been applied — **all of it now has been**:

| Finding | Design consequence | State |
|---|---|---|
| Offloading (§2.1) | The central objection; was unmentioned anywhere in 2,700 lines | Added as `DESIGN.md` **§0.3.1**, with the "better system → stronger effect" mechanism stated and three responses that do not refute it |
| Generation effect (§2.2) | §2.5's automation needed the load-reduction argument made, not assumed | Argued explicitly in **§2.5**, including where automation should therefore stop |
| External aids (§2.3) | Better citation for push-not-pull than the reward-learning inference | Swapped in **§0.2**; rating moved Weak → Moderate |
| Digital interventions (§2.4) | Evidence-backed components were peripheral *and shipped last* | **§8.8 rebuilt**: review, intents and activation moved M5 → **M2** |
| Behavioural activation (§5) | Large effect, software-deliverable, entirely absent | Added as **§3.9**, scoped as activity scheduling with annotation, explicitly not therapy |
| Review unspecified | The best-evidenced component was one table row | Specified in **§2.8**: session shape, selection order, engagement-tuned scheduling |
| Notification cost (§2.5) | Cap should be batching by default; unattended cost never weighed | **§2.6** batches by default; §2.4.1's ruling stands with the cost now on the ledger |
| Comorbidity + anxiety-from-failure (§3) | Indicts the trailing rate that replaced streaks, and graded review outcomes | Completion metrics **off by default** (§3.6); `review` outcome is now shown/engaged/deferred (**§2.7**) |
| Medication correction (§5) | "Largest effects are not software" overstated | Softened in **§0.0** with the actual effect sizes |
| No papers read | The largest limit on this whole base, omitted from §0 | Now the **first** item in §0.0's limits |
| M&O failed replication (§2.6) | Removes an objection to typing-first capture | No edit needed |

Two claims the design corrected in §0 and left standing elsewhere were also fixed: §2.4's "single
best-evidenced ADHD-specific intervention" (false on three counts by §1.3 above) and the opening
line's "the part the evidence says is actually broken".

**The pattern worth watching.** Every correction from the first two review rounds was
*subtractive* — withdraw a claim, add a brake, cap a scope, downgrade a rating — which makes a
document more careful about what it says while leaving untouched what it builds. The M2 reordering
and §3.9 are the first *additive* changes. A future reviewer should check whether that held.

---

## 8. Quick-start for a fresh session

1. Single user, self-reported ADHD + depression + anxiety. Building a personal knowledge system;
   `DESIGN.md` is the design, currently unimplemented (no code exists).
2. The user prefers direct, compressed communication, pushes back on hand-waving, and asked for
   adversarial review twice. Do not soften conclusions to be agreeable.
3. Read §0 before citing anything. Sourcing is search summaries, not papers.
4. §6 is the case against. It was collected on purpose. Use it.
5. Not clinical advice, and not a substitute for their clinicians.
