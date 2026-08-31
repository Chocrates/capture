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
    var note by mutableStateOf<NoteSummary?>(null)
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
            is Screen.Health -> load { conditions = archive.health() }
            is Screen.Note -> load { note = archive.note(to.name) }
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
