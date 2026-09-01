package dev.capture

import androidx.compose.foundation.clickable
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.Card
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TextField
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.launch
import uniffi.capture_ffi.AgendaEntry
import uniffi.capture_ffi.Condition
import uniffi.capture_ffi.Named
import uniffi.capture_ffi.NoteSummary
import uniffi.capture_ffi.Peer
import uniffi.capture_ffi.Pending
import uniffi.capture_ffi.Recording

/**
 * The timeline: everything, newest first, grouped by the day it belongs to.
 *
 * Grouped by the `day` the archive supplied, never by one computed here. A
 * note captured at 23:40 in one place must not move to a different day when
 * read in another (§2.3), and deciding that on this side would be a second
 * answer to a question that already has one.
 */
@Composable
fun Timeline(notes: List<NoteSummary>, onOpen: (String) -> Unit) {
    if (notes.isEmpty()) {
        Empty("Nothing captured yet.")
        return
    }
    LazyColumn(Modifier.fillMaxSize()) {
        // A fold rather than groupBy, to keep the order the archive gave us:
        // sorting by day here would re-decide the ordering.
        var lastDay: String? = null
        items(notes) { note ->
            if (note.day != lastDay) {
                lastDay = note.day
                Text(
                    note.day,
                    style = MaterialTheme.typography.labelLarge,
                    fontWeight = FontWeight.Bold,
                    modifier = Modifier.padding(start = 16.dp, top = 16.dp, bottom = 4.dp),
                )
            }
            NoteRow(note, onOpen)
        }
    }
}

@Composable
private fun NoteRow(note: NoteSummary, onOpen: (String) -> Unit) {
    Column(
        Modifier.fillMaxWidth().clickable { onOpen(note.slug) }.padding(16.dp, 8.dp)
    ) {
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            Text(note.at, style = MaterialTheme.typography.labelSmall)
            Text(note.alias ?: note.slug, style = MaterialTheme.typography.labelSmall)
        }
        Text(
            // Absence is meaningful and is not an empty string. A withheld note
            // says which of the two states it is in, because one of them can
            // still be cancelled and the other cannot.
            note.text ?: when {
                note.redacted -> "(destroyed)"
                note.redactionQueued -> "(queued for destruction — can still be cancelled)"
                else -> "(no readable text)"
            },
            style = MaterialTheme.typography.bodyLarge,
        )
    }
    HorizontalDivider()
}

/** One note in full, and everything that can be done to it. */
@Composable
fun NotePage(note: NoteSummary?, pending: Pending?, recording: Recording?, model: Model) {
    if (note == null) {
        Empty("Loading.")
        return
    }
    Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState())) {
        Column(
            Modifier.padding(16.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            Text(note.alias ?: note.slug, style = MaterialTheme.typography.titleMedium)
            Text("${note.day} ${note.at}", style = MaterialTheme.typography.labelSmall)
            Text(
                note.text ?: when {
                    note.redacted -> "Destroyed. The content is gone."
                    note.redactionQueued -> "Withheld while the destruction is pending."
                    else -> "(no readable text)"
                },
                style = MaterialTheme.typography.bodyLarge,
            )
            if (note.mutedUntilMs != null) {
                Text("Muted.", style = MaterialTheme.typography.labelMedium)
            }
            if (note.pinned) Text("Pinned.", style = MaterialTheme.typography.labelMedium)
            recording?.let { Listen(it, model) }
        }
        HorizontalDivider()
        Actions(note, pending, model)
    }
}

/**
 * A recording, and whether it is here to be played.
 *
 * "Not on this device" is an ordinary state, not a fault: a phone evicts
 * aggressively (§5.6) and the bytes come back on the next round with a device
 * that still holds them. Saying that is different from saying the recording is
 * missing, and a person deleting the note because it looked broken would be
 * acting on the difference.
 */
@Composable
private fun Listen(recording: Recording, model: Model) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var playing by remember { mutableStateOf(false) }

    Column {
        Text(
            recording.durationMs
                ?.let { "A recording, ${it / 1000u} seconds" }
                ?: "A recording",
            style = MaterialTheme.typography.labelMedium,
        )
        if (!recording.here) {
            Text(
                "Not on this device yet. It will arrive on the next sync with " +
                    "a device that has it.",
                style = MaterialTheme.typography.labelSmall,
            )
        } else {
            TextButton({
                if (playing) {
                    Playback.stop()
                    playing = false
                } else {
                    scope.launch {
                        val bytes = model.bytesOf(recording.name)
                        if (bytes == null) {
                            // Held a moment ago and not now: evicted between
                            // the read and the tap, which is unlikely and not
                            // impossible.
                            Toast.short(context, "Those bytes are no longer on this device.")
                        } else {
                            playing = true
                            Playback.play(context, bytes) { playing = false }
                        }
                    }
                }
            }) { Text(if (playing) "Stop" else "Play") }
        }
    }
}

@Composable
fun Search(hits: List<NoteSummary>, onQuery: (String) -> Unit, onOpen: (String) -> Unit) {
    var query by remember { mutableStateOf("") }
    Column(Modifier.fillMaxSize()) {
        TextField(
            value = query,
            onValueChange = { query = it; onQuery(it) },
            modifier = Modifier.fillMaxWidth().padding(16.dp),
            singleLine = true,
        )
        if (query.isNotBlank() && hits.isEmpty()) Empty("Nothing matches.")
        LazyColumn { items(hits) { NoteRow(it, onOpen) } }
    }
}

/**
 * What is on a day.
 *
 * Three bands, and the first is not a subdivision of the second: an all-day
 * entry is a floating date with no position on an hour axis, and a task that is
 * *due* on a day is a third thing again (§4.4, §3.6).
 */
@Composable
fun Agenda(day: String, entries: List<AgendaEntry>) {
    Column(Modifier.fillMaxSize()) {
        Text(day, style = MaterialTheme.typography.titleMedium, modifier = Modifier.padding(16.dp))
        if (entries.isEmpty()) {
            Empty("Nothing on this day.")
            return@Column
        }
        LazyColumn {
            items(entries) { entry ->
                Column(Modifier.fillMaxWidth().padding(16.dp, 6.dp)) {
                    Text(
                        when {
                            entry.due -> if (entry.done) "due · done" else "due"
                            entry.allDay -> "all day"
                            else -> ""
                        },
                        style = MaterialTheme.typography.labelSmall,
                    )
                    Text(entry.text ?: entry.slug, style = MaterialTheme.typography.bodyLarge)
                }
                HorizontalDivider()
            }
        }
    }
}

/**
 * The wiki: named pages, grouped by what they sit under.
 *
 * The parent comes as a field on each page rather than as a tree, so there is
 * nothing here that can get a cycle or an orphan wrong.
 */
@Composable
fun Wiki(pages: List<Named>, onOpen: (String) -> Unit) {
    if (pages.isEmpty()) {
        Empty("No named pages yet. A name is what makes a note a page.")
        return
    }
    val known = pages.mapNotNull { it.alias }.toSet()
    LazyColumn(Modifier.fillMaxSize()) {
        items(pages) { page ->
            Column(
                Modifier.fillMaxWidth().clickable { onOpen(page.alias ?: page.slug) }
                    .padding(start = if (page.parent != null && known.contains(page.parent)) 32.dp else 16.dp,
                             end = 16.dp, top = 8.dp, bottom = 8.dp)
            ) {
                Text(page.alias ?: page.slug, style = MaterialTheme.typography.bodyLarge)
                if (page.parent != null) {
                    Text("under ${page.parent}", style = MaterialTheme.typography.labelSmall)
                }
            }
            HorizontalDivider()
        }
    }
}

/**
 * What the archive would like someone to know.
 *
 * Says so when there is nothing, rather than showing an empty box. An empty
 * surface is ambiguous between "nothing is wrong" and "nothing was checked",
 * and that ambiguity is a cost the system would be manufacturing for free.
 */
@Composable
fun Health(
    conditions: List<Condition>,
    peers: List<Peer>,
    notifier: String?,
    me: String,
    onNotifier: (String?) -> Unit,
    onSync: () -> Unit,
    onPair: () -> Unit,
) {
    val attention = conditions.filter { it.tier == "attention" }
    val knowing = conditions.filter { it.tier != "attention" }

    LazyColumn(Modifier.fillMaxSize().padding(16.dp)) {
        item { Text("Needs attention", style = MaterialTheme.typography.titleSmall) }
        if (attention.isEmpty()) {
            item { Text("Nothing needs your attention.", Modifier.padding(vertical = 8.dp)) }
        }
        items(attention) { Card(Modifier.fillMaxWidth().padding(vertical = 4.dp)) {
            Text(it.text, Modifier.padding(12.dp))
        } }

        if (knowing.isNotEmpty()) {
            item { Text("Worth knowing", style = MaterialTheme.typography.titleSmall,
                        modifier = Modifier.padding(top = 16.dp)) }
            items(knowing) { Card(Modifier.fillMaxWidth().padding(vertical = 4.dp)) {
                Text(it.text, Modifier.padding(12.dp))
            } }
        }

        item {
            Text(
                "Devices",
                style = MaterialTheme.typography.titleSmall,
                modifier = Modifier.padding(top = 24.dp),
            )
        }
        if (peers.isEmpty()) {
            item {
                Text(
                    "No other devices yet. Pair one to sync.",
                    Modifier.padding(vertical = 8.dp),
                )
            }
        }
        item {
            Column(Modifier.fillMaxWidth().padding(vertical = 6.dp)) {
                Text("$me (this phone)", style = MaterialTheme.typography.bodyMedium)
                Text(
                    if (notifier == me) "Says things when a cue fires" else "",
                    style = MaterialTheme.typography.labelSmall,
                )
                if (notifier != me) {
                    TextButton({ onNotifier(me) }) { Text("Notify me here") }
                }
            }
        }
        items(peers) { peer ->
            Column(Modifier.fillMaxWidth().padding(vertical = 6.dp)) {
                Text(peer.key, style = MaterialTheme.typography.bodyMedium)
                Text(
                    // What it can watch for, so a notifier is chosen among
                    // devices that can deliver (§7.5). "Not known yet" rather
                    // than an empty list read as a capability: a device that
                    // has not been synced with has said nothing, which is not
                    // the same as saying it watches nothing.
                    if (peer.observes.isEmpty()) {
                        "Has not said what it can watch for"
                    } else {
                        "Watches for ${peer.observes.joinToString(", ")}"
                    },
                    style = MaterialTheme.typography.labelSmall,
                )
                if (notifier == peer.key) {
                    Text(
                        "Says things when a cue fires",
                        style = MaterialTheme.typography.labelSmall,
                    )
                } else {
                    TextButton({ onNotifier(peer.key) }) { Text("Notify me there") }
                }
                // When, never "fresh" or "stale" (§6.6). The device that
                // sleeps most is the one that must not imply currency, and
                // this is that device.
                Text(
                    peer.lastSeen?.let { "Last reached $it" } ?: "Never reached from here",
                    style = MaterialTheme.typography.labelSmall,
                )
                if (peer.missed > 0u) {
                    Text(
                        "${peer.missed} attempt(s) since",
                        style = MaterialTheme.typography.labelSmall,
                    )
                }
            }
        }
        item {
            // Said out loud, because the difference is the whole of §8.9: a
            // value nobody chose is a rule that a better rule can replace, and
            // a value somebody chose is theirs. Writing the first down as the
            // second destroys the distinction permanently.
            Text(
                notifier?.let { "Chosen by you." }
                    ?: "Nobody has chosen. Whichever device sees a cue says so.",
                style = MaterialTheme.typography.labelSmall,
                modifier = Modifier.padding(top = 8.dp),
            )
            if (notifier != null) {
                TextButton({ onNotifier(null) }) { Text("Stop choosing") }
            }
            Row {
                TextButton(onSync) { Text("Sync now") }
                TextButton(onPair) { Text("Pair a device") }
            }
        }

        item {
            Text(
                "Signatures, body hashes and origin chains are not checked here.",
                style = MaterialTheme.typography.labelSmall,
                modifier = Modifier.padding(top = 24.dp),
            )
        }
    }
}

@Composable
fun Empty(what: String) {
    Column(Modifier.fillMaxSize().padding(24.dp)) {
        Text(what, style = MaterialTheme.typography.bodyMedium)
    }
}
