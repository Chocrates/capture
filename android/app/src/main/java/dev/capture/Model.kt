package dev.capture

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import uniffi.capture_ffi.AgendaEntry
import uniffi.capture_ffi.Archive
import uniffi.capture_ffi.Condition
import uniffi.capture_ffi.ListLine
import uniffi.capture_ffi.Named
import uniffi.capture_ffi.NoteSummary
import uniffi.capture_ffi.Peer
import uniffi.capture_ffi.Recording
import uniffi.capture_ffi.Intent as Plan
import uniffi.capture_ffi.Resurfaced
import uniffi.capture_ffi.Watching
import uniffi.capture_ffi.Pending
import uniffi.capture_ffi.Ticked
import uniffi.capture_ffi.TaskLine
import uniffi.capture_ffi.Entity
import uniffi.capture_ffi.NoteKind

/** Which surface is showing. */
sealed interface Screen {
    data object Timeline : Screen
    data object Search : Screen
    data class Agenda(val day: String) : Screen
    data object Wiki : Screen
    data object Health : Screen
    data class Note(val name: String) : Screen
    data object Pairing : Screen
    data object Review : Screen
    data object Plans : Screen
    /**
     * What is outstanding.
     *
     * A route of its own rather than a band inside the agenda. §3.6 separates
     * time from state — an event occupies time and has none, a task has state
     * and occupies no time — and folding tasks into the day's view would
     * re-conflate exactly what that table exists to keep apart. An undated
     * task has no day to be folded onto at all, and it is the ordinary case.
     */
    data object Tasks : Screen
    /**
     * A person, a project or a tag, and the notes naming it.
     *
     * Addressed by the mention as written — `@sarah`, `^acme` — because that is
     * what a reader taps, and what the sigil means is decided in the store
     * rather than here.
     */
    data class Entity(val mention: String) : Screen
    /**
     * A list, by its tag.
     *
     * Reached from a tag rather than from a tab. §3.7 makes a collection a
     * saved query, and a tag is where a query starts — and the tab row is
     * already one tab past what a phone's width holds.
     */
    data class ListOf(val tag: String) : Screen
}

/**
 * What is on screen, and nothing about what any of it means.
 *
 * Every value here came from the archive already decided: which day a note
 * belongs to, how a date reads, what is due, what needs attention. This holds
 * the results and the loading state, which is the whole of what a phone's
 * presentation layer is entitled to know.
 *
 * Reads run on the IO dispatcher because they open a database. They are also
 * cheap enough that nothing here caches: a cache is a second copy of the
 * projection's answer, and the projection is already the cached form of the
 * log.
 */
class Model(private val archive: Archive) : ViewModel() {

    // Tasks rather than the timeline, matching where the application opens.
    // A first frame drawn on one surface and replaced by another is a flicker
    // somebody has to look past on every launch.
    var screen by mutableStateOf<Screen>(Screen.Tasks)
        private set

    var timeline by mutableStateOf<List<NoteSummary>>(emptyList())
        private set

    /** Open tasks, banded by the archive rather than by this screen. */
    var tasks by mutableStateOf<List<TaskLine>>(emptyList())
        private set

    /**
     * Captures nothing has read yet, so a screen can say it may be incomplete.
     *
     * §3.1 puts the parse after the commit, so a capture is not a promotion
     * until something reads it — and until then a task list is missing entries
     * for a reason that has nothing to do with what is outstanding. The
     * foreground settle means this should almost always be zero, which is
     * exactly why it is worth showing when it is not: a non-zero count here is
     * the settle having failed, and that is a thing to see rather than a thing
     * to look at an empty list about.
     *
     * Read-only on the archive's side, so asking costs nothing and works when
     * the writer does not.
     */
    var unread by mutableStateOf(0u)
        private set

    /** Everyone and every project the archive knows, for the index. */
    var people by mutableStateOf<List<Entity>>(emptyList())
        private set
    var projects by mutableStateOf<List<Entity>>(emptyList())
        private set

    /** The entity page being read, whole. */
    var entity by mutableStateOf<Entity?>(null)
        private set

    /**
     * What the archive thinks the open note might be, where it was unsure
     * enough not to have acted. Read only on the note page, because answering
     * it costs a parse.
     */
    var suggestion by mutableStateOf<String?>(null)
        private set
    var hits by mutableStateOf<List<NoteSummary>>(emptyList())
        private set

    /** What produced [hits], so the same question can be asked again. */
    private var asked: String = ""
    var agenda by mutableStateOf<List<AgendaEntry>>(emptyList())
        private set
    var pages by mutableStateOf<List<Named>>(emptyList())
        private set
    var list by mutableStateOf<List<ListLine>>(emptyList())
        private set
    var conditions by mutableStateOf<List<Condition>>(emptyList())
        private set
    var peers by mutableStateOf<List<Peer>>(emptyList())
        private set

    /**
     * The device chosen to say things, if one was chosen.
     *
     * `null` is not "nothing notifies" — it is "nobody has decided", and §8.9
     * keeps those apart deliberately. A default written down on somebody's
     * behalf can never be told apart afterwards from a choice they made.
     */
    var notifier by mutableStateOf<String?>(null)
        private set

    /** What each capture surface has cost, measured rather than claimed. */
    var costs by mutableStateOf<List<String>>(emptyList())
        private set

    /** This device's own key, to know whether the chosen one is us. */
    val me: String by lazy { archive.deviceKey() }
    var offered by mutableStateOf<List<Resurfaced>>(emptyList())
        private set
    var nagging by mutableStateOf<List<String>>(emptyList())
        private set
    var plans by mutableStateOf<List<Plan>>(emptyList())
        private set
    /**
     * How much of the time this device may see where it is.
     *
     * Asked of the platform on every resume rather than remembered: it can be
     * taken away in Settings while the application is in the background.
     */
    var located by mutableStateOf(Watching.NEVER)
        private set
    var note by mutableStateOf<NoteSummary?>(null)
        private set

    /** The recording the note on screen refers to, where it refers to one. */
    var recording by mutableStateOf<Recording?>(null)
        private set

    /** The destruction waiting on the note on screen, where one is waiting. */
    var pending by mutableStateOf<Pending?>(null)
        private set

    /** Where a pairing has got to, if one is happening. */
    var pairing by mutableStateOf<PairingState>(PairingState.Idle)
        private set

    /** The last thing that went wrong, for saying so rather than swallowing it. */
    var trouble by mutableStateOf<String?>(null)
        private set

    var busy by mutableStateOf(false)
        private set

    /**
     * Tick an item off a list, or put it back.
     *
     * One call for every surface a list can be drawn on — the list screen, a
     * timeline row, a search hit. There is one meaning of completion and one
     * place that decides it; what differs is only which surface is now holding
     * a stale copy, and that is answered by looking at which one is showing.
     *
     * Reloads rather than editing the row in place. The archive decides what a
     * list holds — an item may have been ticked on another device since this
     * screen drew — and a phone that patched its own copy would be a second
     * opinion about what is still to buy.
     */
    fun tick(tag: String, item: String, done: Boolean) {
        if (tag.isEmpty()) return
        load {
            when (val outcome = archive.tick(tag, item, done)) {
                is Ticked.Ambiguous -> trouble =
                    "${outcome.count} items on #$tag read \"$item\". " +
                        "Ticking matches an item by its text, so these cannot be told apart."
                is Ticked.NoSuchItem -> trouble = "Nothing on #$tag reads \"$item\"."
                // Done and Already are both the state asked for, and saying
                // so twice would be noise on a surface somebody is tapping
                // through a shop.
                else -> trouble = null
            }
            reread()
            if (screen is Screen.Search) hits = archive.search(asked, 50u)
        }
    }

    /**
     * Tick one item of a capture, by where it sits in it.
     *
     * What every surface here should use. A row on screen already knows which
     * one a thumb landed on, so there is nothing to match by words — and two
     * items reading the same were untickable until this existed, because
     * matching by words found both and refused. "milk, milk" is what somebody
     * writes when two shops are meant.
     *
     * The position goes into the log with the tick, so correcting a word later
     * does not discard it.
     */
    fun tickAt(name: String, ord: UInt, done: Boolean) = load {
        when (archive.tickAt(name, ord, done)) {
            is Ticked.NoSuchItem ->
                trouble = "That item is no longer on this capture."
            // Nothing to say for the rest. Ambiguous cannot happen here — that
            // is the whole point of addressing a row rather than its words.
            else -> trouble = null
        }
        reread()
        if (screen is Screen.Search) hits = archive.search(asked, 50u)
    }

    /**
     * Where this session has been, oldest first.
     *
     * A list rather than a single previous screen, because the gesture is
     * expected to walk back the whole way — timeline, note, list, back, back,
     * back. Keeping one step meant the second press left the application
     * entirely, from somewhere that plainly had a history.
     *
     * Not persisted. A back stack is about the session in front of you, and
     * restoring one across a process death would offer to return somewhere the
     * person had already left.
     */
    private val history = mutableStateListOf<Screen>()

    /** Whether there is anywhere to go back to. */
    val canGoBack: Boolean
        get() = history.isNotEmpty()

    /**
     * Back, to wherever this came from.
     *
     * Returns false where there is nowhere left, which is the caller's signal
     * to let the platform have the gesture — at the bottom of the stack, back
     * means leave, and an application that swallowed it there would be one you
     * could not get out of.
     */
    fun back(): Boolean {
        val previous = history.removeLastOrNull() ?: return false
        show(previous)
        return true
    }

    fun go(to: Screen) {
        // Consecutive duplicates are not history. Tapping the tab you are
        // already on is not a move, and recording it would make the first back
        // press appear to do nothing.
        if (to != screen) {
            history.add(screen)
        }
        show(to)
    }

    /**
     * Draw a screen and load what it needs, without touching the history.
     *
     * Separate from [go] so that going back re-reads the archive rather than
     * restoring what was on screen when you left. The archive may have changed
     * underneath — a sync landed, another device ticked something off — and a
     * back gesture that showed a remembered copy would be presenting stale
     * state as current.
     */
    private fun show(to: Screen) {
        screen = to
        when (to) {
            is Screen.Timeline -> load { timeline = archive.timeline(100u) }
            is Screen.Tasks -> load {
                tasks = archive.tasks()
                unread = archive.unread()
                // For the banner. The screen the application opens on is the
                // one place a problem is certain to be seen.
                conditions = archive.health()
            }
            is Screen.Agenda -> load { agenda = archive.agenda(to.day) }
            is Screen.Wiki -> load {
                pages = archive.named()
                people = archive.people()
                projects = archive.projects()
            }
            is Screen.Entity -> load { entity = archive.entity(to.mention) }
            is Screen.ListOf -> load { list = archive.list(to.tag) }
            is Screen.Health -> load {
                conditions = archive.health()
                peers = archive.peers()
                notifier = archive.notifier()
                costs = archive.costs()
            }
            is Screen.Note -> load {
                note = archive.note(to.name)
                pending = archive.pendingRedaction(to.name)
                recording = archive.recordingOf(to.name)
                suggestion = archive.suggestion(to.name)
            }
            is Screen.Search -> hits = emptyList()
            is Screen.Pairing -> pairing = PairingState.Idle
            is Screen.Plans -> load { plans = archive.intents() }
            // Opening a session writes — the archive records that these were
            // offered, which is what expands the interval. So it happens when
            // somebody goes to review, and not on every navigation past it.
            is Screen.Review -> load {
                val session = archive.review()
                offered = session.items
                nagging = session.nagging
            }
        }
    }

    fun search(query: String) {
        if (query.isBlank()) {
            hits = emptyList()
            return
        }
        asked = query
        load { hits = archive.search(query, 50u) }
    }

    fun today(): String = archive.today()

    /**
     * A note's text, split into ordinary runs and mentions.
     *
     * Asked of the archive rather than worked out here. Which runs are mentions
     * is a rule the store owns — `sarah@example.com` names nobody, and a phone
     * scanning for sigils itself would get that wrong and then drift.
     */
    fun pieces(text: String): List<uniffi.capture_ffi.TextPiece> = archive.pieces(text)

    // --- mutations -----------------------------------------------------------
    //
    // §8.4 makes this the first-party surface and §8.10 scoped it to every
    // mutation the desktop offers. The reason is §2.6: suppressing something
    // is meant to be cheaper than destroying it, and that gradient inverts
    // the moment muting needs a laptop and destroying does not.
    //
    // Each one re-reads rather than editing the copy on screen. The archive
    // may have done more than was asked — a mute settles, an alias moves a
    // page — and a screen patched locally would show the request instead of
    // the result.

    fun edit(name: String, text: String) = after(name) { archive.edit(name, text) }

    fun tag(name: String, tag: String) = after(name) { archive.tag(name, tag) }

    fun untag(name: String, tag: String) = after(name) { archive.untag(name, tag) }

    fun setAlias(name: String, alias: String?) = after(name) { archive.setAlias(name, alias) }

    fun setParent(name: String, under: String?) = after(name) { archive.setParent(name, under) }

    fun pin(name: String, pinned: Boolean) = after(name) { archive.pin(name, pinned) }

    fun mute(name: String) = after(name) { archive.mute(name) }

    fun setDone(name: String, done: Boolean) = after(name) { archive.setDone(name, done) }

    /**
     * Say what a capture is.
     *
     * Pinned by the archive, so no later reparse takes it back. Offered on the
     * note page and deliberately nowhere else: a control on a scrolling row is
     * a mis-tap away from writing a correction that outlives every future
     * grammar.
     */
    fun setKind(name: String, kind: NoteKind) = after(name) { archive.setKind(name, kind) }

    /** Make something a task, with a due date or without one. */
    fun asTask(name: String, due: String?) = after(name) { archive.asTask(name, due) }

    /** Take the reading the archive offered but would not act on by itself. */
    fun accept(name: String) = after(name) { archive.accept(name) }

    /**
     * Insert the character a capture was missing to be a list.
     *
     * Rewrites what somebody wrote, where [accept] annotates and leaves the
     * words alone. The surface says which character and where, because making
     * the two look alike would teach that accepting a suggestion never touches
     * the text — which would then be false.
     */
    fun fixList(name: String) = after(name) { archive.acceptNearMiss(name) }

    // --- entities ------------------------------------------------------------
    //
    // Re-read the page rather than patching it. A merge changes which notes the
    // page holds and a rename changes what every other page calls it, so a
    // locally-edited copy would show the request instead of the result.

    fun renameEntity(mention: String, display: String) = afterEntity(mention) {
        archive.renameEntity(mention, display)
    }

    /** Join two entities. The surviving page is the one this stays on. */
    fun mergeEntities(from: String, into: String) = afterEntity(into) {
        archive.mergeEntities(from, into)
    }

    fun bindBacking(mention: String, note: String?) = afterEntity(mention) {
        archive.bindBacking(mention, note)
    }

    private fun afterEntity(mention: String, work: suspend () -> Unit) = load {
        work()
        entity = archive.entity(mention)
    }

    fun redact(name: String) = after(name) { archive.redact(name) }

    /**
     * Call off a queued destruction.
     *
     * The id is fetched rather than remembered from the ordering call: this
     * process may not be the one that ordered it, and on a phone it usually is
     * not.
     */
    fun cancelRedaction(name: String) = after(name) {
        archive.pendingRedaction(name)?.let { archive.cancelRedaction(it.id) }
    }

    /** Do something to a note, then re-read what it actually became. */
    private fun after(name: String, work: suspend () -> Unit) = load {
        work()
        note = archive.note(name)
        pending = archive.pendingRedaction(name)
        // A kind that just changed is a kind the recognizer is no longer
        // suggesting, so the offer has to be re-asked rather than left up.
        suggestion = archive.suggestion(name)
        // The lists behind this note are now stale in a way that is visible
        // the moment the back gesture happens.
        timeline = archive.timeline(100u)
    }

    /**
     * Show a code and wait for a device to answer it.
     *
     * Two steps rather than one: the code has to be on screen while the window
     * is open, and a single call that returned it when pairing completed would
     * hand it over at the one moment it is useless.
     */
    fun offerPairing(addresses: List<String>) {
        viewModelScope.launch {
            pairing = try {
                val offer = withContext(Dispatchers.IO) { archive.offerPairing() }
                pairing = PairingState.Showing(offer.code(), addresses.firstOrNull())
                // The wait is a person walking to another machine, so it holds
                // no lock and blocks nothing but its own thread.
                PairingState.Joined(withContext(Dispatchers.IO) { offer.wait() })
            } catch (e: Exception) {
                PairingState.Failed(e.message ?: e.toString())
            }
        }
    }

    /** Type a code shown on another device. */
    fun joinPairing(addr: String, code: String) {
        viewModelScope.launch {
            // Said before the socket opens, not after. Everything below blocks,
            // and a screen that only changes when it finishes is a screen that
            // looks broken for as long as it takes to fail.
            pairing = PairingState.Trying
            pairing = try {
                // Named by key, as every other surface names a device. The
                // address is where it was a moment ago.
                PairingState.Joined(withContext(Dispatchers.IO) { archive.joinWithCode(addr, code) })
            } catch (e: Exception) {
                PairingState.Failed(e.message ?: e.toString())
            }
        }
    }

    /** The bytes of the recording on screen, if this device holds them. */
    suspend fun bytesOf(name: String): ByteArray? =
        withContext(Dispatchers.IO) { archive.recording(name) }

    /** What became of an offered note. Two outcomes, and neither is a grade. */
    fun reviewed(name: String, outcome: String) = load {
        archive.reviewed(name, outcome)
        // Taken off the list, because it has been answered. Not re-opened:
        // that would spend another session.
        offered = offered.filterNot { it.slug == name }
    }

    /**
     * Choose which device says things, or stop choosing.
     *
     * `null` reverts to the rule, which is a third state and not the same as
     * never having chosen — a later, better rule should follow a revert and
     * should not overrule a choice.
     *
     * A place cue is unaffected either way. It fires where it is observed,
     * because "you are at the hardware shop" delivered to a desktop at home is
     * delivered to an empty room (§2.4.1).
     */
    fun chooseNotifier(device: String?) = load {
        archive.setNotifier(device)
        notifier = archive.notifier()
    }

    /**
     * Close a device's membership.
     *
     * §5.3's remedy for a lost device, from the device that is left. It was
     * only ever half reachable: retiring a lost *phone* from the desktop
     * always worked, and a lost *desktop* left this device holding the archive
     * with the missing key still a member and nothing able to revoke it.
     * `Archive.retire` was written for exactly that and nothing called it.
     *
     * The archive matches the key, refuses an ambiguous prefix and authors the
     * event. What is decided here is only that the list is re-read afterwards,
     * so the row that was acted on stops offering to send alerts to a device
     * that is no longer a member.
     */
    fun retire(key: String) = load {
        archive.retire(key)
        peers = archive.peers()
        conditions = archive.health()
        notifier = archive.notifier()
    }

    /** Say whether the platform is currently letting this device see where it is. */
    fun located(watching: Watching) {
        located = watching
        load {
            archive.locationPermitted(watching)
            // The plans list carries whether each can be watched here, so it
            // is stale the moment a permission changes.
            if (screen is Screen.Plans) plans = archive.intents()
        }
    }

    /**
     * Watch for a place.
     *
     * Coordinates in micro-degrees, because that is what goes into the event
     * body: integers are exact and hash into an event id, where a float would
     * make the id depend on two devices agreeing bit-for-bit about a decimal.
     */
    fun watchPlace(latE6: Int, lonE6: Int, radiusM: UInt, then: String) = load {
        archive.watchPlace(latE6, lonE6, radiusM, then)
        plans = archive.intents()
    }

    fun retract(id: String) = load {
        archive.retractIntent(id)
        plans = archive.intents()
    }

    /** Where this device can be reached, for the other one to dial. */
    fun addresses(): List<String> = archive.addresses()

    /**
     * Settle whatever has come due, then re-read what is on screen.
     *
     * The re-read is the part that was missing, and the gap was visible: a
     * capture is committed without parsing it (§3.1 — the gesture must never
     * wait for the recognizer), so what reads it is the *next* session, which
     * is this one. `onResume` fired this and the screen's own read as two
     * coroutines, and the read usually won — so a capture made a moment ago
     * drew as plain text, and stayed that way until somebody navigated
     * somewhere and back.
     *
     * Nothing failed, and that is what made it hard to see: the note was
     * promoted the whole time and the surface was showing an answer from
     * before it happened.
     */
    /**
     * Whether anything is scheduled to speak.
     *
     * Asked before requesting permission to speak: an archive with no dated
     * captures has no reason to want it, and asking anyway spends a request
     * that is then refused before it meant anything.
     */
    /** Whether anything scheduled named a time of day rather than just a day. */
    fun hasTimedAlerts(): Boolean {
        val month = 30L * 24 * 60 * 60 * 1000
        return runCatching { archive.due(month).any { it.timed } }.getOrDefault(false)
    }

    fun hasAlerts(): Boolean {
        val month = 30L * 24 * 60 * 60 * 1000
        return runCatching { archive.due(month).isNotEmpty() }.getOrDefault(false)
    }

    fun settle() = load {
        archive.settle()
        reread()
    }

    /** Re-read the current screen from the archive. */
    private suspend fun reread() {
        when (val here = screen) {
            is Screen.Timeline -> timeline = archive.timeline(100u)
            is Screen.Tasks -> {
                tasks = archive.tasks()
                unread = archive.unread()
            }
            is Screen.Agenda -> agenda = archive.agenda(here.day)
            is Screen.ListOf -> list = archive.list(here.tag)
            is Screen.Wiki -> {
                pages = archive.named()
                people = archive.people()
                projects = archive.projects()
            }
            is Screen.Entity -> entity = archive.entity(here.mention)
            is Screen.Note -> note = archive.note(here.name)
            is Screen.Plans -> plans = archive.intents()
            else -> {}
        }
    }

    /**
     * Re-read whatever is on screen.
     *
     * A sync happens in a worker this has no handle on, so the screen that
     * asked for one is stale the moment it succeeds — and a peer list showing
     * "has not said what it can watch for" *after* a successful round reads as
     * the round having failed. Cheap enough to do unconditionally: these are
     * indexed reads against a store that is already open.
     */
    fun refresh() = go(screen)

    /**
     * Re-read once the round has had time to finish.
     *
     * Not a poll and not a progress bar: `WorkManager` owns the job and this
     * process cannot be told when it ends. One re-read after the window a
     * round is given is enough to turn "nothing happened" into what actually
     * happened, and the screen is re-read on navigation anyway.
     */
    fun refreshSoon() {
        viewModelScope.launch {
            kotlinx.coroutines.delay((SyncWorker.WINDOW_SECONDS.toLong() + 2) * 1000)
            refresh()
        }
    }

    /**
     * What an error says, without the generator's punctuation.
     *
     * uniffi renders an error variant's message as `field=value`, so every
     * refusal this application has ever shown began with `reason=` — a Rust
     * field name, on a phone screen, in front of a sentence written for a
     * person. The wording still comes from Rust; this only undoes the prefix.
     */
    private fun said(e: Exception): String =
        (e.message ?: e.toString()).replaceFirst(Regex("^[a-z_]+="), "")

    private fun load(work: suspend () -> Unit) {
        viewModelScope.launch {
            busy = true
            trouble = null
            try {
                withContext(Dispatchers.IO) { work() }
            } catch (e: Exception) {
                // Said, not swallowed. A blank screen where a list should be
                // is indistinguishable from an empty archive, and one of those
                // is a bug.
                trouble = said(e)
            } finally {
                busy = false
            }
        }
    }
}
