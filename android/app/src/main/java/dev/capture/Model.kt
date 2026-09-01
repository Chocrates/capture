package dev.capture

import androidx.compose.runtime.getValue
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
import uniffi.capture_ffi.Named
import uniffi.capture_ffi.NoteSummary
import uniffi.capture_ffi.Peer
import uniffi.capture_ffi.Intent as Plan
import uniffi.capture_ffi.Resurfaced
import uniffi.capture_ffi.Watching
import uniffi.capture_ffi.Pending

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

    var screen by mutableStateOf<Screen>(Screen.Timeline)
        private set

    var timeline by mutableStateOf<List<NoteSummary>>(emptyList())
        private set
    var hits by mutableStateOf<List<NoteSummary>>(emptyList())
        private set
    var agenda by mutableStateOf<List<AgendaEntry>>(emptyList())
        private set
    var pages by mutableStateOf<List<Named>>(emptyList())
        private set
    var conditions by mutableStateOf<List<Condition>>(emptyList())
        private set
    var peers by mutableStateOf<List<Peer>>(emptyList())
        private set
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

    fun go(to: Screen) {
        screen = to
        when (to) {
            is Screen.Timeline -> load { timeline = archive.timeline(100u) }
            is Screen.Agenda -> load { agenda = archive.agenda(to.day) }
            is Screen.Wiki -> load { pages = archive.named() }
            is Screen.Health -> load {
                conditions = archive.health()
                peers = archive.peers()
            }
            is Screen.Note -> load {
                note = archive.note(to.name)
                pending = archive.pendingRedaction(to.name)
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
        load { hits = archive.search(query, 50u) }
    }

    fun today(): String = archive.today()

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
            pairing = try {
                // Named by key, as every other surface names a device. The
                // address is where it was a moment ago.
                PairingState.Joined(withContext(Dispatchers.IO) { archive.joinWithCode(addr, code) })
            } catch (e: Exception) {
                PairingState.Failed(e.message ?: e.toString())
            }
        }
    }

    /** What became of an offered note. Two outcomes, and neither is a grade. */
    fun reviewed(name: String, outcome: String) = load {
        archive.reviewed(name, outcome)
        // Taken off the list, because it has been answered. Not re-opened:
        // that would spend another session.
        offered = offered.filterNot { it.slug == name }
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

    /** Settle whatever has come due. Called when the app comes forward. */
    fun settle() = load { archive.settle() }

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
                trouble = e.message ?: e.toString()
            } finally {
                busy = false
            }
        }
    }
}
