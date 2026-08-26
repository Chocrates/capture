## 1. Workspace, tooling, and the purity boundary

- [x] 1.1 Create the cargo workspace: `crates/brain-core`, `crates/brain-store`, `apps/brain-cli`, `apps/brain-capture`, `apps/brain-desktop`. Pin dependency versions.
- [x] 1.2 Decide the store path and config resolution (`~/brain` with a `$BRAIN_HOME` override for tests). All three binaries must agree, and tests cannot run without an override.
- [x] 1.3 Define the crate-wide error type and its mapping to the exit codes in §7. Retrofitting typed errors later is a large refactor; decide now.
- [x] 1.4 Enforce `brain-core` purity structurally: `#![no_std]` + `alloc` so `SystemTime` and `std::fs` are not nameable; `clippy.toml` `disallowed-methods` for `Instant::now`, `rand`, `getrandom`; `cargo-deny` bans on `tokio`, `rusqlite`, `getrandom` entering its tree. **This replaces the earlier idea of a test that "fails to compile if a clock is threaded in", which is not expressible in Rust.**
- [x] 1.5 Set up CI running the test suite on Linux and macOS. The frozen-encoding claims are cross-platform claims and are otherwise unverified.
- [x] 1.6 Add `tracing` to the applications (not to `brain-core`) so ingest rejections, quarantine reasons and purge phases have somewhere to go before `brain status` exists.

## 2. Frozen primitives — decide before a single event is written

Everything in this group is permanent once the first capture lands.

- [x] 2.1 **Byte-specify the canonical encoding using `minicbor` with explicit field indices.** Decode **rejects** non-canonical input rather than re-canonicalising — `bhash` is computed over canonical bytes, so acceptance would only move the failure further from its cause, and rejection means no second valid encoding of an event can exist. Note the earlier "deterministic + fixed field order" phrasing was self-contradictory; indexed fields resolve it. Unknown-field *preservation* is a storage property (events are stored as the exact bytes `bhash` commits to and are never re-encoded), so the codec need only skip them on decode.
- [x] 2.2 Define the event envelope: `id, dev, seq, hlc, kind, tgt, prev, occ, chain, bhash, body, sig`. Document as frozen at the top of the module.
- [x] 2.3 **Byte-specify the body of every M1 event kind.** Bodies are hashed into `bhash` into `id` and signed, so each is frozen on first use and `envelope_version` does not cover them. Must settle: whether the ambient envelope lives inside the `capture` body (it must, or `notes.tz` has no source); how `untag` names the adds it retracts; whether `redact` targets are note ids or event ids and how scope is discriminated; `mute` scope and expiry; what `resolve` identifies. The nonce is **field index 0**, not "the first field" — with indexed encoding you do not control physical position.
- [x] 2.4 Fix the M1 event-kind list and treat the names as permanent: `capture, edit, tag, untag, alias, pin, mute, resolve, redact, redact_cancel, tombstone, device_add, origin_claim, setting`. **`origin_claim` and `setting` were missing** — without the first, restore locks the device out of its own log; without the second, `brain status` provenance is trivially satisfied by doing nothing. Decide explicitly whether `tombstone` ships in M1 or is dropped from the list.
- [x] 2.5 Implement `bhash = blake3(canonical body)` with a 16-byte random nonce supplied by the caller. **Property test:** `bhash == blake3(canonical_encode(body))` for arbitrary bodies.
- [x] 2.6 Implement `id = blake3(canonical header ‖ bhash)` and Ed25519 signing over `id`, with the signing key passed in. **Property test:** mutating any header field changes the id; a signature over one header does not verify against another.
- [x] 2.7 **Implement the HLC**: byte layout, comparison (wall ms, then counter, then origin id, with the byte ordering stated), the emit ceiling, the receive rule `l := max(l, incoming)`, and **persistence of the last emitted value across restart**. The persistence part is what bites on one device in week one, when a clock correction or reboot would otherwise produce descending timestamps.
- [x] 2.8 Implement the per-origin `chain` link and its verification. **Property test:** a chain walk detects removal, truncation, substitution, and a forked parent. It does **not** detect reordering — `chain` is a property of the links, not of storage order, and arrival order is arbitrary by construction.
- [x] 2.9 Implement origin identity `dev = H(pubkey ‖ incarnation)` with the incarnation as a random 128-bit value supplied by the caller.
- [x] 2.10 **Write the golden vector fixture**: logical event → exact encoded bytes → exact id, committed to the repo and checked by CI on every target. This is the only way "identical across platforms" is enforceable from a single-target test run.
- [x] 2.11 **Settle the slug algorithm and freeze it**: word count, transliteration, and the pinned Unicode table versions (which form part of the algorithm version, because a table upgrade would silently change existing slugs on the next rebuild). Commit slug test vectors.
- [x] 2.12 **Implement the credential predicate** as a pure function in `brain-core`, inside the frozen slug algorithm: high-entropy tokens, long base64/hex runs, credential keywords adjacent to a value, PEM headers, connection strings. Flagged captures take a date-derived slug. Tuned to over-flag — a false positive costs readability, a false negative is a permanent leak into other notes' link text. Not the recogniser; it must not wait for M2. Commit predicate vectors alongside the slug vectors.

## 3. Identity and store creation

Moved ahead of everything that appends. Nothing in groups 5–8 can run without it.

- [x] 3.1 Implement store creation: directory layout, `events.db`, `state.db`, `ops.db`, `blobs/`, `export/`, `config/`.
- [x] 3.2 Implement keypair generation and the identity record, with file mode 0600 on the private key.
- [x] 3.3 Implement incarnation minting and the identity record's highest-written-sequence field.
- [x] 3.4 Implement the self-signed roster root, valid only as the first event of an origin with no predecessor in a log with no other root. A second root is **quarantined**, not rejected — its signature verifies.
- [x] 3.5 Implement `origin_claim` emission when a new incarnation is minted, binding the new origin to the existing roster key.
- [x] 3.6 Implement the spool as a **plain 10 MB preallocated file** at `~/brain/spool`, written full at install, with length-prefixed records. **Not a SQLite table** — it would need the disk that just filled, and the grow-then-delete reservation trick conflicts with the `secure_delete` redaction uses.

## 4. Fold and conflict rules — pure, no storage

- [x] 4.1 **Build the synthetic multi-origin event-set generator.** Causally coherent sets: `prev` pointing at events in the set, `tgt` at real roots, per-origin `seq` gap-free and chain-linked, HLCs consistent with causality, valid signatures — plus perturbations for out-of-order delivery, missing predecessors, unknown kinds and concurrent branches. **Tasks 4.3–4.7 and 5.4 all consume this; it is the largest hidden item in the plan and the mitigation the design promises for the rules M1 cannot otherwise exercise.**
- [x] 4.2 Define the fold's output as a pure data structure with a canonical encoding, distinct from anything written to SQLite. The determinism property is only meaningful about a value, not about a database file.
- [x] 4.3 Implement the fold over all M1 kinds, sorting by HLC. **Property test:** shuffle any generated event set, fold, compare — byte-identical output.
- [x] 4.4 Implement the note version graph and reachability-based concurrency detection, retaining edges for quarantined events. **Property test:** a multi-edit branch against a single-edit branch yields exactly one divergence; a sequential chain of any length yields none.
- [x] 4.5 Implement tags as an add-wins observed-remove set. **Property test:** commutativity, idempotence and associativity over arbitrary tag/untag sequences with arbitrary observed sets.
- [x] 4.6 Implement the full conflict rule set: body divergence retains both, scalars and `alias` LWW by HLC with history, edit beats tombstone, **redaction wins at any HLC in any arrival order**, and redactions are retained indefinitely and applied to events arriving later.
- [x] 4.7 Implement slug derivation per 2.11, with deterministic collision resolution by note-identity ordering and an identity-only slug for redacted notes. **Property test with a test-only narrowed suffix width** so collisions actually occur — at the real width the birthday probability is negligible and the test proves nothing.

## 5. Storage and ingest

- [x] 5.1 Create the `events.db` schema: events, heads (contiguous), pending. WAL; `synchronous=FULL` set **on every connection**, since it is per-connection while `journal_mode` is per-database.
- [x] 5.2 Create the `state.db` schema — including `meta` (schema and slug versions), `roster`, `roster_origins`, `device_caps`, `settings`, `redact_queue`, `fts_map`, `note_fts` with `contentless_delete=1`, and the note/version/tag/entity/ambient/link/quarantine tables. Enable `bundled` on the SQLite dependency; the system library may predate `contentless_delete`.
- [x] 5.3 Create the `ops.db` schema: rejected, peers, redact_intent, snapshots, export watermark, activity tracking. The spool is deliberately not here — see 3.6.
- [x] 5.4 Implement atomic append: `seq` allocation, row insert and `heads` update in **one transaction**. **Test:** the three operations are never in autocommit.
- [x] 5.5 Implement contiguous-heads maintenance with the `pending` set. **Property test:** ingesting a generated multi-origin stream in random order yields the same heads as in-order ingest. *This is the one test that exercises the contiguous-heads rule before sync exists.*
- [x] 5.6 Implement the ingest pipeline in order: signature, `blake3(body) == bhash` (skipped where the body is already redacted), origin key lookup against the roster, then foldability. First three failures go to `rejected` keyed by id, never claiming a position or advancing `heads`. Fourth goes to `quarantine` in place.
- [x] 5.7 Implement the `(dev, seq)` collision rule: identical id is an idempotent no-op; **differing id is a loud quarantine**, never a silent skip. Reachable in M1 via overlapping export replay.
- [x] 5.8 Implement the roster bootstrap: the roster is a fold output that ingest depends on, so a rebuild constructs it first and **ingest pauses rather than accepting unverified events** while it is unavailable.
- [x] 5.9 Implement quarantine re-evaluation on `device_add`/`origin_claim` ingest, on chain-predecessor arrival, and on every rebuild.
- [x] 5.10 Implement incremental fold with a **per-origin** watermark and per-note recomputation. A single scalar watermark either skips late arrivals or forces a full rebuild on every reconnection.
- [x] 5.11 **Property test: incremental fold ≡ full fold** for any event set and any ingest partition. Two implementations of the same rules with nothing checking they agree is how a silently-wrong projection happens.
- [x] 5.12 Implement FTS index maintenance during the fold, keyed through `fts_map` so a hit resolves to a note and a single note's entries can be deleted.
- [x] 5.13 Implement rebuild with a **generation counter in `meta`**: write generation n+1 alongside, bump the counter, readers reopen on their next transaction. **Never rename over an open database** — other processes keep reading the old inode, and the `-wal`/`-shm` sidecars are path-named, so the new file can pair with the old file's WAL.
- [x] 5.14 Implement **single-writer election** via an advisory lock on the store. The overlay normally holds it; if absent, the first CLI invocation takes it for its lifetime. Non-writers open read-only and submit writes over the overlay socket. This gives one owner for the fold, rebuilds, and the due-work queue — WAL prevents interleaved transactions, not two processes independently folding the same events.
- [x] 5.15 Set `synchronous=FULL` as a **connection-open invariant on every writing connection**, not on the capture path. `journal_mode` persists in the file; `synchronous` is per connection, so setting it in one binary leaves the durability guarantee silently false in the others. Also set `busy_timeout` everywhere.
- [x] 5.16 Implement **read-only direct access** for read commands and `brain verify`, with no dependency on the writer being up. Diagnostics that fail when the system is unhealthy are unusable exactly when needed.
- [x] 5.17 Implement reader change-detection by polling `heads` on a short interval. Socket push is a later optimisation, not a correctness requirement.

## 6. Crash-safety harness

- [x] 6.1 **Build the crash-injection harness before anything claims crash safety.** A subprocess supervisor that SIGKILLs at randomised points and checks invariants on restart. Note this tests process death, not power loss — the page cache survives, so it does not exercise `synchronous=FULL`.
- [x] 6.2 Run the harness against append: no gaps, no partial rows, `heads` consistent with the log.
- [x] 6.3 Run it against spool drain, rebuild, and the redaction purge. The purge case kills a process between clearing the projection and destroying the payload — a window microseconds wide in normal use, widened by `BRAIN_PURGE_PAUSE_MS` so a kill can land in it. Verified by mutation: removing the intent marker makes the test fail with a note reporting its content destroyed while the payload is still there.

## 7. CLI

- [x] 7.1 Scaffold the `brain` binary. A bare unrecognised word errors and suggests the capture form; it never silently captures.
- [x] 7.2 Implement `brain +`: `--stdin`, non-tty stdin detection, an in-process line editor when interactive with no input, `--` literal form, and refusal to store empty content.
- [x] 7.3 **Ship the capture keybinding, not a shell function.** bash `bind -x` reading `READLINE_LINE`; zsh ZLE widget reading `$BUFFER`. A function cannot work — by the time it runs the shell has already split, expanded and truncated the line. The gesture is *type the text, press the chord*, not *type `b` then the text*.
- [x] 7.4 **Test 7.3 through a pty harness** driving `bash -i` and `zsh -i`. History expansion of `!` and `^` only happens interactively, so a non-interactive test passes while the human path stays broken. Verify `#`, `>`, `!`, `~`, `^`, `*`, `$VAR` and backticks survive byte-identically.
- [x] 7.5 Implement read commands — `timeline`, `show`, `search`, `links` — each with `--json`.
- [x] 7.6 Implement write commands: `edit`, `tag`, `alias`, `pin`, `mute`, `resolve`.
- [x] 7.7 Implement ops commands: `log <slug>`, `status` (with default-vs-set provenance), `quarantine`, `rejected`, `fold --rebuild`.
- [x] 7.8 Implement `export` and `restore [--ignore-redactions-after <t>]`. Without these the export capability is a feature nothing can trigger.
- [x] 7.9 Implement `brain verify`: recompute every id, check every signature, check `blake3(body) == bhash` where a body exists, walk every origin chain. Rejected events are summarised, not treated as failure.
- [x] 7.10 Implement exit codes: 0 success, 1 usage, 2 not found, 3 integrity failure.

## 8. Capture

- [x] 8.1 Implement the capture path: accept text, build the envelope with injected nonce/HLC/key, append, return the slug. Never validate, never reject.
- [x] 8.2 Implement the ambient envelope collector behind a platform trait: capture timezone, working directory, git repo and branch; foreground app and window title **only where the platform exposes them**. Verified absent on the target (GNOME 46 Wayland: no protocol, `Shell.Eval` disabled, Xwayland `_NET_ACTIVE_WINDOW` returns `0x0`). Refuse the Xwayland value explicitly — it is a plausible-looking zero, not an answer. **Never read the clipboard.**
- [x] 8.3 Implement the spool: write on append failure, assign `seq` at drain, preserve capture time and timezone. Loud failure when the spool itself is exhausted. **Drain is event-driven, not scheduled** — on the next successful append and on startup. A timer retrying against a still-full disk is pure waste, and disk recovery is invisible to a clock.
- [x] 8.4 Build `apps/brain-capture`: resident egui process, pre-warmed window, single text field, Enter commits, Escape discards.
- [x] 8.5 Implement the hotkey platform trait with **runtime detection** in order: portal → GNOME gsettings → X11 grab → none. Default chord named in one place. Report which mechanism was selected in `brain status`.
- [x] 8.6 Implement the **GNOME gsettings** backend: installer registers a custom keybinding running a thin client that signals the resident overlay over a unix socket and exits. *This is the mechanism that works on the target machine — GNOME 46 exposes no `GlobalShortcuts` portal.*
- [x] 8.7 Implement the `GlobalShortcuts` **portal** backend, selected only when the interface is actually present (KDE today). Needs D-Bus and a runtime in the overlay — permitted there, forbidden in `brain-core`. **Written against the specification and never run against a desktop that implements it** — the target is GNOME 46, which publishes the portal without this interface. The parts checkable without one (reply-path derivation, token legality, trigger syntax) are pure functions with tests; the D-Bus conversation itself is unexercised, and the first KDE session it meets is its first real test.
- [x] 8.8 Implement the X11 `XGrabKey` backend, used **only under a genuine X11 session**. Explicitly refuse to select it under Xwayland: the grab fires only while an X11 window has focus, which is the silent-dead-gesture failure 8.9 exists to catch. Refused in two places — `hotkey::select` will not choose it, and the backend checks again before grabbing. **Not exercised against a real X11 session**; the logic (modifier masks, every lock-modifier combination, keysym lookup, the Wayland refusal) is unit-tested, and taking the grab is not. Verified on the target only in the negative: no grab is attempted under Wayland.
- [x] 8.9 **Decided: macOS is not an M1 target.** The Accessibility grant does not persist across restarts without a signed binary, so an unsigned build produces a chord that works until reboot and then silently stops — the precise failure 8.10 exists to catch, shipped as the default experience. M1 targets Linux, GNOME/Wayland primary. The platform trait in 8.5 isolates registration, so macOS is additive later rather than a refactor.
- [x] 8.10 Implement registration confirmation ("press it now"), and dead-gesture detection after the defined idle period using the activity tracking from 5.3. **Load-bearing, not ceremonial** — a spawn-based binding has more ways to be registered-but-not-firing than a grab does.
- [x] 8.11 Instrument capture latency permanently (callback → first frame) into a local histogram, reported by `brain status`. **Not a CI gate.** In-process portions measured on the target and recorded in `docs/capture-latency.md`: ~3ms to spawn the client and reach the socket, <2ms to draw — about 5ms for the share this project controls. **End-to-end keypress-to-cursor is still unmeasured**; it needs filming the screen and keyboard together, and the document says how.

## 9. Redaction and muting

- [x] 9.1 Implement `mute` at note, tag and date-range scope, with expiry, applied to every surfacing path — the timeline shows a muted note in place with a marker, search leaves it out. **Entity scope is deferred**: M1 has no entity extraction, so there is nothing to scope to; the `MuteScope` enum takes a new variant without touching the ones that exist.
- [x] 9.2 Implement `redact`: whole-note default scope, per-event option, 64-target cap.
- [x] 9.3 Implement the ingest-side rate limit — 256 targets per hour per origin, then held for explicit confirmation.
- [x] 9.4 Implement the destruction queue **in `state.db`**, derived from the redaction events, with `execute_after` from the redaction's own HLC rather than local receipt. **Test:** ingesting a redaction whose window has elapsed destroys immediately rather than granting a fresh window.
- [x] 9.5 Implement `redact_cancel` with the same retention as `redact`, and reporting when a cancellation arrives after destruction.
- [x] 9.6 Implement the ordered purge with the durable intent marker: intent → purge projection and FTS → destroy body → checkpoint → clear intent. Use `secure_delete` rather than `VACUUM`; `VACUUM` needs free space equal to the database and fails exactly when the disk is full.
- [x] 9.7 Implement the pre-redaction snapshot with 30-day expiry, local only, never exported, and **skipped entirely for credential-flagged notes**.
- [x] 9.8 Implement `brain redact`, `--force` (suppresses the prompt only — cap, rate limit, window and snapshot all remain), `--cancel <id>`, and `--cancel --all-since <t>`.
- [x] 9.9 Implement the **due-work queue** in `state.db`: destruction deadlines, snapshot expiry, dead-gesture detection. Drained on **every process startup** (the floor) and on a low-frequency timer in the **resident overlay** (the latency path). No daemon and no systemd timer — a fourth binary is one more thing that can be silently down, and an external timer puts scheduling where `brain status` cannot see it.
- [x] 9.10 Report **when due work was last processed** in `brain status`. Without it, a long-dead overlay means redactions sitting undestroyed while the interface reports them complete — the failure that put the queue in the projection rather than in local state.

## 10. Export and restore

- [x] 10.1 Define the export file format and naming, and record the watermark in `ops.db` (not `state.db`, which is disposable and would replay every export on rebuild).
- [x] 10.2 Implement incremental export — **manual only**, never scheduled by the application. Scheduling it would invite the belief that backups are handled, and §1.2 is explicit that offsite is out of scope. Export, **excluding credential-flagged notes by default**, reporting the excluded count, and supporting a one-action flag clear that includes the note in the next export. Exclusion must never be silent — the predicate over-flags, so silent omission would trade a leak for missing backups.
- [x] 10.3 Apply retained redactions when writing new export files; surface the notice that prior exports must be pruned by hand. Note this means export rows may have no body, and ingest must skip the body check for them.
- [x] 10.4 Implement restore as replay through the ordinary ingest path, minting a fresh incarnation and emitting `origin_claim` before any append, spooling captures until that completes.
- [x] 10.5 Implement `restore --ignore-redactions-after <t>` as an explicit non-default mode.

## 11. First run

- [x] 11.1 Four-state startup detection with a **directional** consistency check. DESIGN.md previously specified a symmetric one — corrected there, with the reason: the event commits before the watermark is written, so a lagging watermark is the ordinary post-crash state and minting an incarnation for it would start a fresh origin after every crash.
- [x] 11.2 Implement the unresolved-state behaviour: block appends, accept captures into the spool, resolve automatically where the direction is determinable and ask only where it is not.
- [x] 11.3 Implement compiled-in defaults with no `setting` events written on first run, and provenance display in `brain status`.
- [x] 11.4 Implement the empty state: name the capture gesture; swap to the CLI instruction when the gesture is unavailable; no sample content.
- [x] 11.5 `install.sh` — builds, installs to `~/.local/bin`, creates the store (spool preallocated by `brain init`), and **prints** the shell snippet and chord registration rather than performing them. Editing a shell rc or a desktop's keybindings silently is a change to the user's environment that they cannot then tell the program to stop making.

## 12. Browse

- [x] 12.1 Tauri scaffolded with a static frontend — **no Node toolchain**. The frontend is plain HTML, CSS and JavaScript served from `ui/`, so there is no bundler, no npm tree and no build step between editing a file and seeing it. Linux needs webkit2gtk-4.1, now installed.
- [x] 12.2 Implement the timeline: day grouping by capture timezone, capture-time order within a day, human edits bump within their own day, machine activity never moves a note. **The human-versus-machine distinction is a fold rule about event kind, not a rendering rule** — it belongs in the core.
- [x] 12.3 Implement the four render states — incomplete, redacted with date, diverged with expansion, muted.
- [x] 12.4 Implement the ambient context strip with click-to-filter.
- [x] 12.5 Implement the note page: full text, context, tags, outgoing links, backlinks, version history when diverged. Entities are enrichment and stay empty in M1 — do not build a section that cannot populate.
- [x] 12.6 Implement link extraction and resolution **in the fold**, recording both the written name and the note bound at authoring time, rendering broken rather than retargeting on mismatch. Not in the shell.
- [x] 12.7 Implement search over the FTS index, ranked by relevance.
- [x] 12.8 Implement the M1 health surface: tiers, cause-specific wording, acknowledgeable tier-one items, affirmative healthy state, no claim about anything unverified. Peer-related conditions are deferred with sync.
- [x] 12.9 Implement the GUI mutation surface: edit, tag, alias, pin, mute, resolve. The GUI is the first-party surface (§8.4) — management should not require a terminal.
- [x] 12.10 Implement GUI redaction with a confirmation that **enumerates what will be destroyed** and has **no default-focused accept**. A dialog is more reflex-dismissible than a terminal prompt, and this is the only destructive confirmation in the system.
- [x] 12.11 Implement the pending-redaction countdown with a **working cancel**. The only live element in the interface; a safety net requiring a terminal during the window is not one.
- [x] 12.12 Verify no on-screen control creates a note.

## 13. Verification and acceptance

- [x] 13.1 Assemble the `brain-core` property suite: encoding determinism against golden vectors, identity, chain, HLC ordering, fold determinism, incremental ≡ full, divergence detection, tag convergence, slug collision resolution.
- [x] 13.2 Write CLI integration tests asserting against `--json`, covering each spec scenario.
- [x] 13.3 Run the crash harness across append, purge, spool drain and rebuild; assert no gaps, no partial rows, and no state in which the index matches destroyed content.
- [x] 13.4 Run `brain verify` over a synthetic log containing redacted, quarantined and rejected events; confirm exit codes.
- [x] 13.5 Confirmed on the target: GNOME 46 on Wayland, no `GlobalShortcuts` portal, registered via a gsettings custom keybinding. Chord pressed, client spawned, socket reached, overlay shown — and the dead-gesture condition cleared itself, which is the check confirming its own signal works.
- [ ] 13.6 Use it for a day. Fix what makes capture feel slow or uncertain before starting M2.
