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
import uniffi.capture_ffi.Pending

/** Which surface is showing. */
sealed interface Screen {
    data object Timeline : Screen
    data object Search : Screen
    data class Agenda(val day: String) : Screen
    data object Wiki : Screen
    data object Health : Screen
    data class Note(val name: String) : Screen
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
    var note by mutableStateOf<NoteSummary?>(null)
        private set

    /** The destruction waiting on the note on screen, where one is waiting. */
    var pending by mutableStateOf<Pending?>(null)
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
