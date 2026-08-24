## Context

`DESIGN.md` is the design of record for the whole system; this document covers only how M1 gets
built and which of its decisions are irrevocable.

The constraint that shapes everything here: **M1 writes the first events, and events are immutable
forever.** Sync (M3) is where a second replica first disagrees with this one, but by then every
event M1 wrote is already permanent. So the identity, encoding and sequencing rules must be right
now, in a milestone where nothing is replicating and nothing appears to depend on them.

Three rounds of adversarial review found the majority of their defects in exactly these rules,
including several introduced *as fixes* for earlier defects. `DESIGN.md` records each rejected
approach alongside the accepted one; those records are load-bearing and should be read before
changing anything in `brain-core`.

## Goals / Non-Goals

**Goals**

- A single-device system usable every day: capture, browse, search, redact, export.
- The durable layer correct enough that M3 can replicate against it without a format break.
- `brain-core` I/O-free and async-free, so the fold and conflict rules are exhaustively testable
  with no runtime, no clock, and no filesystem.
- Integrity claims that are runnable (`brain verify`) rather than asserted.

**Non-Goals**

- Any networking. No sockets, no discovery, no async runtime in M1.
- Typed promotion, recurrence, calendar — M2.
- Review, intents, activation — M2. These are the evidence-backed components and they are early by
  intent, but they need M2's recurrence machinery.
- Enrichment of any kind — M5.
- At-rest encryption — deferred as a cluster; M1 stores plaintext and says so.

## Decisions

### D1: `id` commits to a body *hash*, not the body

`id = blake3(canonical header ‖ bhash)`, `bhash = blake3(canonical body)`.

*Alternative considered:* `id = blake3(whole envelope including body)`. Simpler, and it was the
first draft. Rejected because redaction destroys the body, after which the id can never be
recomputed and integrity checking is permanently broken for that row — the rows most in need of an
audit trail.

*Consequence that must not be forgotten:* the id then authenticates the body's hash rather than the
body, so `blake3(body) == bhash` becomes a **mandatory ingest check**. Without it a relaying device
can substitute content that still verifies. This is cheap and easy to omit, which is why it is a
requirement and a `brain verify` check rather than an implementation note.

### D2: Every body begins with a 16-byte random nonce

*Alternative considered:* no nonce, on the grounds that `bhash` is "just a hash".

Rejected because it is a *commitment to content that redaction destroys*, surviving on every
replica and in every export. Redaction exists for passwords and third-party details — short,
low-entropy strings — so an adversary with any export file could confirm a guess by hashing
candidates. The nonce makes the commitment hiding at a cost of 16 bytes per event, and every
property redaction claims is preserved.

### D3: Origin identity is `H(pubkey ‖ random incarnation)`

*Alternative considered:* an incrementing incarnation counter. Rejected: the counter must be read
from the artifact that was just lost, so restoring twice, or restoring from a stale export, mints
the same origin and collides at every position. An incarnation is a nonce, not a version.

*Consequence:* roster membership is keyed by **public key**, not origin, so a restored device is
still a member. M1 has one device and one key, so this costs nothing now and prevents restore from
becoming a lockout in M3.

### D4: `heads[dev]` is the highest contiguous sequence

*Alternative considered:* `max(seq)`, which is the natural thing to write.

Rejected because arrival is out-of-order by construction once M3 exists, and a maximum silently
loses data: an interrupted range transfer that already delivered a later event leaves `heads`
advanced past events never requested again. M1 has no out-of-order arrival, which is exactly why
this must be built correctly now — nothing will exercise it until M3, when the log is already full.

### D5: Unauthenticated and unfoldable events are handled differently

Bad signature or `bhash` mismatch → a side table keyed by `id`, no `(dev, seq)` claim, no `heads`
advance, never relayed. Unknown kind, malformed body, unadmitted origin → stored in place,
quarantined, relayed.

*Alternative considered:* one uniform "quarantine everything" rule, which was an earlier draft.
Rejected because it lets an unauthenticated event squat a sequence position, after which the
genuine event at that position can never be accepted anywhere.

The distinction is precise: an event whose signature verifies is provably authored by the origin
whose position it occupies, so it cannot be squatting. One whose signature does not verify proves
nothing at all.

### D6: The fold takes the event set and nothing else

No wall clock, no current roster, no local configuration. Time-dependent values — whether an
occurrence has lapsed, completion rates — are computed at read time and are not columns.

*Two rejected drafts, both of which looked like fixes:* quarantining future-dated events from the
projection (made the fold clock-dependent, so three devices showed three states), and clamping
incoming timestamps at ingest (destroyed the causality guarantee the logical clock exists to
provide, so a later edit could sort before what it edited).

Enforced by property test: shuffle any event set, fold, compare.

### D7: `state.db` is disposable; there is no migration code

Schema change → bump the version → re-fold. Rebuild runs in the background into a new file and
swaps.

*Alternative considered:* conventional migrations. Rejected as unnecessary — the log is the truth
and the projection is a cache — and as a permanent tax: a decade of migration steps that must each
keep working.

### D8: Three stores, split by derivability, and the purge order that follows

`events.db` (durable truth), `state.db` (derived, disposable), `ops.db` (local, not derivable, not
replicated). The split rule is **derivability, not category** — anything reconstructible from the
log belongs in `state.db` and is rebuilt for free; only what exists solely on this device goes in
`ops.db`.

That rule places the redaction *intent marker* in `ops.db` (a local crash-recovery fact) and the
redaction *queue* in `state.db` (derived from the redaction events and their cancellations). Putting
the queue in `ops.db` alongside the marker is the intuitive mistake and it is a real bug: after a
restore the log replays, notes are marked redacted, the projection is purged — and nothing ever
destroys the bodies, because nothing drives the destruction. The replica then holds every redacted
plaintext indefinitely while reporting all redactions complete.

`events.db` and `state.db` cannot be committed atomically in WAL mode, so redaction is an ordered
sequence replayed from the marker on restart: intent → purge projection and index → destroy body →
clear intent. The order is chosen so that every interruption leaves the safe state — the index is
destroyed before the content, so no crash window leaves searchable text pointing at destroyed
content.

Destruction itself is `secure_delete` plus a WAL checkpoint rather than `VACUUM`. `VACUUM` rewrites
the whole database and needs free space equal to its size, which fails precisely when the disk is
full — the case where redaction matters most on a constrained machine.

### D11: The hybrid logical clock is as frozen as the identity construction

`hlc` is a signed header field, the total order the fold sorts by, and the tiebreak for every
last-writer-wins rule. Its layout, its comparison, and its emit and receive rules are permanent.

*Two rejected drafts, both of which looked like fixes:* clamping an incoming timestamp on ingest
(destroys causality — a later edit can sort before the version it edits, silently losing every LWW
race for the duration of the skew), and excluding future-dated events from the fold (makes the
projection depend on the local clock, so the same log renders differently on three machines).

The correct rules are: bound at **emission** only, **adopt** on receipt, and persist the last
emitted value across restart — that last part being the one that bites on a single device in week
one, when a clock correction or a reboot would otherwise produce descending timestamps.

### D12: Entropy, clocks and filesystem access are injected, never taken

`brain-core` is I/O-free, async-free and clock-free, but several things assigned to it are none of
those: the 16-byte body nonce, the 128-bit incarnation, key generation, the HLC stamp, and
timezone resolution.

All of them are **values passed in at the boundary** — `Envelope::build(body, HlcStamp, Nonce,
&SigningKey)` — with `brain-store` and the applications supplying them. Only the fold, the conflict
rules and slug derivation are genuinely pure, and those are the things that must stay pure.

*Alternative considered:* a `Clock` trait object threaded through the core. Rejected — it is exactly
D6's rejected draft wearing an interface, and once it exists nothing stops the fold taking it.

Enforcement is structural rather than aspirational: `no_std` on `brain-core` so `SystemTime` and
`std::fs` are not nameable, clippy `disallowed-methods` for the rest, and a dependency ban so
`getrandom`, `rusqlite` and `tokio` cannot enter its tree.

### D9: The CLI capture path is a shell function, not an alias

Every sigil the recogniser uses is also a shell metacharacter. A command alias silently truncates
at `#`, redirects at `>`, expands globs, interpolates variables, and executes backticks — in the
one path that must never alter input.

*Alternative considered:* requiring the user to quote. Rejected as the same failure in a politer
form: the user must still notice, at the moment they can least afford to.

The function reads the raw input line and passes it on stdin. An argument form with `--` remains
for scripts.

### D10: Slug derivation is frozen and versioned

Derived from the **root capture body** — never the current text, never machine output — plus a
50-bit suffix, using a byte-specified algorithm with an explicit version.

*Alternatives considered:* 16-bit suffix (rejected — collides at realistic volumes, and since the
column is unique a collision aborts the fold, which is also the recovery and migration path);
title-derived slugs (rejected — titles change when enrichment runs, breaking hand-written links);
unversioned derivation (rejected — improving the algorithm would silently change existing slugs on
the next re-fold).

Redacted notes take an identity-derived slug, which both keeps the row reproducible after its text
is destroyed and stops the name leaking the words it was built from.

## Risks / Trade-offs

- **Global hotkey registration is the highest-risk item in the milestone.** Wayland needs a
  portal that is unevenly implemented; macOS needs Accessibility permission and a signed binary; an
  X11 grab can be taken by another application afterwards. → Verify against the actual target
  compositor before building anything else in the overlay; confirm registration by requiring the
  user to press the key once; detect a gesture that never fires and raise it. The CLI is a genuine
  fallback capture path, so failure degrades rather than blocks.

- **The durable rules are unexercised in M1.** Contiguous heads, quarantine/rejected separation and
  incarnation minting have almost no observable effect on one device, so defects will not surface
  until M3 — by which time the log is full of events written under them. → Property-test them
  directly in `brain-core` against synthetic multi-origin event sets, rather than waiting for real
  replication.

- **Plaintext at rest.** M1 stores the log, the projection and the export directory unencrypted.
  On a machine without full-disk encryption this is the weakest point in the system, and the export
  directory is the easiest target because it is the thing users point backup tools at. → Stated
  plainly rather than implied; verify platform disk encryption is on; revisit as a cluster.

- **Redaction cannot reach everything.** Prior exports, offsite copies, text copied by hand into
  other notes, and flash-storage remanence are all outside its reach. → Enumerated as explicit
  limits rather than left for a user to discover.

- **Two UI stacks** (egui overlay, Tauri desktop) is real duplication. → Justified less by latency
  than by isolation: the capture path stays independent of the main application's crashes, updates
  and memory footprint.

- **M1 cannot answer the project's central question.** It shows whether capture happens, not
  whether the system helps. The components with evidence behind them arrive in M2. → Do not read a
  quiet M1 as a verdict on the concept; `DESIGN.md` §8.8 sets a 90-day observation window and
  measures cost rather than counts.

## Migration Plan

Nothing exists, so there is no migration in the usual sense. What matters is the ordering that
keeps the irrevocable decisions ahead of the code that depends on them:

1. `brain-core` first: canonical encoding, envelope, identity, signing, fold, conflict rules. No
   I/O. Property tests before any storage exists.
2. `brain-store` second: both databases, ingest checks, heads maintenance, rebuild-by-re-fold.
3. `brain-cli` third: it is the harness. `brain verify` lands here and gates everything after.
4. Capture overlay and desktop app last, against a core that is already proven.

Rollback: the log format is the only thing that cannot be rolled back. Everything above it is
rebuilt by re-folding. Before the first real capture, treat the store as disposable; after it,
treat the encoding as frozen.

## Open Questions

- ~~**Wayland global shortcuts** on the target compositor.~~ **Settled.** Checked against the
  machine: Ubuntu / GNOME Shell 46 / Wayland / `xdg-desktop-portal` 1.18.4 exposes **no**
  `GlobalShortcuts` interface, and the GNOME portal backend is not installed. Xwayland does not
  help — an X11 grab fires only while an X11 window has focus, which is the silent-dead-gesture
  case §8.7 exists to catch. Resolution: a platform trait detecting portal → GNOME `gsettings`
  keybinding → X11 grab → none, with the gsettings path being what actually works here. A GNOME
  Shell extension was rejected (separate install, breaks across GNOME versions). See `DESIGN.md`
  §8.5. Note the portal backend needs D-Bus and a runtime **in the overlay** — permitted there;
  the Non-Goal forbids network sockets and a runtime in `brain-core`, not that.
- ~~**Wayland ambient context.**~~ **Settled.** Confirmed unavailable on the target: no Wayland
  protocol for focused-window identity, `org.gnome.Shell.Eval` disabled in release builds, and
  Xwayland's `_NET_ACTIVE_WINDOW` returns `0x0` — which must be explicitly refused rather than
  stored. A Shell extension was rejected for the same reasons as in the hotkey decision.

  Accepted consequence: terminal captures carry working directory, repository and branch; overlay
  captures carry little beyond time. Rather than treat that as a defect, the **terminal keybinding
  is promoted to a primary capture surface** alongside the overlay (`DESIGN.md` §2.2) — it is the
  only surface that can collect context at all here. Two chords on one platform sits awkwardly
  against §0.2's context-stability rule; mitigated by their being chords in different *places*, and
  by the difference in captured context being visible in the timeline rather than hidden.

  The clipboard field is **excluded**, not deferred: silently copying the clipboard into an
  append-only log is how a credential gets into it, and redaction cannot reach already-written
  exports.
- ~~**Which CBOR crate.**~~ **Settled: `minicbor` 2.3, with explicit field indices, and decode
  rejects non-canonical input.** Indices put the wire layout in the type rather than deriving it
  from field names or declaration order, so a Rust rename cannot change an id; it is `no_std` first,
  composing with D12 rather than fighting it; and it is deterministic by construction rather than by
  a flag. `serde_cbor` is unmaintained since 2021; `ciborium` does not sort keys and forks ids
  silently on a `HashMap` or a `flatten`; `cbor2` is the closest runner-up and loses only because
  serde ergonomics are what let encodings drift unnoticed.

  Two things fell out. **Unknown-field preservation is not a codec requirement** — events are stored
  as the exact bytes `bhash` commits to and are never re-encoded, so it is already satisfied and the
  codec need only *skip* unknown fields. And the nonce is **field index 0**, not "the first field";
  with indexed encoding physical position is not something you state. Both are corrected in
  `DESIGN.md` §8.3.
- ~~**Whether the credential heuristic is in M1.**~~ **Settled: yes, as a pure predicate in
  `brain-core`, inside the frozen slug algorithm.** Not the recogniser — it cannot wait for M2,
  because adding the check later would change existing slugs on the next rebuild, which is exactly
  what freezing prevents. Safe to freeze because it may only ever become *more* conservative: notes
  already slugged by words keep their slugs, so tightening never rewrites history. Tuned to
  over-flag; a false positive costs readability, a false negative is an unfixable leak into other
  notes' link text.

  The same signal is reused twice more, since detection happens once at capture: **no pre-redaction
  snapshot** for flagged notes, and **excluded from export by default**. The export rule cuts
  against "data loss is the cardinal sin", so it carries a rail — exclusion is reported with a
  count and the flag clears in one action. The default protects the credential case; the visibility
  protects the false positives the predicate is deliberately generating.
- ~~**What runs on a schedule.**~~ **Settled: a due-work queue in the projection, drained on every
  process startup and on a low-frequency timer in the resident overlay** (`DESIGN.md` §8.7). No
  daemon — a fourth binary is one more thing that can be silently down; no systemd timer — it puts
  scheduling where `brain status` cannot see it. Without the overlay the system falls back to
  startup-only, which costs latency rather than correctness.

  Two riders. `brain status` reports **when due work was last processed**, or a long-dead overlay
  means redactions sitting undestroyed while the UI claims otherwise. And **spool drain is not on
  the queue** — it is event-driven, on the next successful append and on startup, because retrying
  against a still-full disk is waste and disk recovery is invisible to a clock.

  **Export stays manual.** Wiring it to cron is the user's call; scheduling it inside the
  application would invite the belief that backups are handled, which §1.2 explicitly disclaims.
- ~~**Two processes, one store.**~~ **Settled: a single writer elected by an advisory lock, reads
  direct** (`DESIGN.md` §8.8). The overlay normally holds the lock; absent it, the first CLI
  invocation takes it. One elected writer gives one owner for the fold, for rebuilds and for the
  due-work queue — WAL prevents interleaved transactions but not two processes independently folding
  the same events, which is a correctness problem rather than a locking one.

  Read commands and `brain verify` open read-only with no dependency on the writer: diagnostics that
  stop working when the system is unhealthy are unusable at the only moment they matter.

  Two riders. `synchronous=FULL` is a **connection-open invariant**, not a property of the capture
  path — `journal_mode` persists in the file but `synchronous` is per connection, so setting it in
  one binary leaves durability silently false in the others. And rebuild swaps by **generation
  counter**, never by rename: renaming over an open database leaves readers on the old inode, and
  the path-named `-wal`/`-shm` sidecars can end up crossed.

  *Rejected:* making the CLI a pure client that never opens SQLite. It removes the whole problem
  class, but capture would stop working whenever the overlay cannot start — breaking §0.2's
  highest-rated rule at exactly the wrong moment.
- ~~**Spool size and mechanism.**~~ **Settled: a plain 10 MB preallocated file, outside all three
  databases.** SQLite has no preallocation primitive, and the grow-then-delete reservation trick
  conflicts with the `secure_delete` redaction uses — but the decisive point is simpler: a spool
  inside `ops.db` would need the same disk that just filled up, which is the condition it exists to
  survive. A plain file written full at install, with length-prefixed records, is what
  preallocation actually means here.

  10 MB is roughly 50,000 captures, far more headroom than the failure needs. Kept anyway because
  it costs nothing on a modern disk. The real protection is the immediate tier-1 notification the
  moment the spool is in use (§8.7) — capacity only buys time to notice.

  Recorded for later: **voice capture would exhaust any spool size immediately.** M1 is text-only so
  it does not arise, but audio needs its own answer when it lands.
- ~~**Where the Tauri app ends and the CLI begins for M1.**~~ **Settled: full parity. The GUI is the
  first-party surface** — reading, editing, tagging, aliasing, pinning, muting, resolving, and
  redacting, plus the pending-redaction countdown with a working cancel (`DESIGN.md` §8.4).

  This grows M1 beyond a read-only GUI, accepted deliberately: management that requires a terminal
  pushes ordinary gestures — muting the note in front of you — into a second tool, and §2.6 exists
  so that suppressing is *cheaper* than destroying. That gradient inverts if suppression is
  terminal-only.

  The CLI is not demoted; it keeps the two jobs the GUI cannot do — **capture from a terminal**,
  where it is the better surface because it is the only one that collects context (§2.2), and the
  **test harness** (§8.6).

  One rider: the graphical redaction confirmation must **enumerate what will be destroyed and have
  no default-focused accept**. A modal is materially more reflex-dismissible than a terminal prompt,
  and §1.1 warns this is the only destructive confirmation in the system and must not be trained
  away.
