package dev.capture

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TextField
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import uniffi.capture_ffi.NoteSummary
import uniffi.capture_ffi.Pending

/**
 * Everything that can be done to a note, from the note.
 *
 * §8.4 makes the reading surface the first-party one and §8.10 scoped it to
 * every mutation, for a reason that bites harder on a phone than anywhere
 * else: §2.6 exists so that suppressing something is *cheaper* than destroying
 * it, and that gradient inverts the moment muting needs a laptop and
 * destroying does not.
 */
@OptIn(ExperimentalLayoutApi::class)
@Composable
fun Actions(note: NoteSummary, pending: Pending?, model: Model) {
    var asking by remember { mutableStateOf<Ask?>(null) }
    var destroying by remember { mutableStateOf(false) }

    pending?.let {
        // When, not how long is left. A countdown rendered once is wrong
        // within a minute, and wrong in the direction of looking safe.
        Text(
            "Will be destroyed at ${it.at} on ${it.day}.",
            Modifier.padding(start = 16.dp, end = 16.dp, top = 16.dp),
            style = MaterialTheme.typography.labelMedium,
        )
    }

    FlowRow(
        Modifier.fillMaxWidth().padding(16.dp),
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        TextButton({ asking = Ask("Edit", "The new text", note.text ?: "") { model.edit(note.slug, it) } }) {
            Text("Edit")
        }
        TextButton({ asking = Ask("Tag", "A tag", "") { model.tag(note.slug, it) } }) { Text("Tag") }
        TextButton({
            asking = Ask("Rename", "A chosen name; empty clears it", note.alias ?: "") {
                model.setAlias(note.slug, it.ifBlank { null })
            }
        }) { Text("Rename") }
        TextButton({
            // The one filing gesture, and it says what it costs: a page put in
            // the wrong branch is still in the timeline, in search and behind
            // every link (§3.7).
            asking = Ask("Move", "Under which page; empty puts it back at the top", "") {
                model.setParent(note.slug, it.ifBlank { null })
            }
        }) { Text("Move") }
        TextButton({ model.pin(note.slug, !note.pinned) }) {
            Text(if (note.pinned) "Unpin" else "Pin")
        }
        if (note.mutedUntilMs == null) {
            TextButton({ model.mute(note.slug) }) { Text("Mute") }
        }
        if (note.redactionQueued) {
            // The window is the point. While it is open the content is
            // withheld and still there, and this is the only moment it can be
            // called back.
            TextButton({ model.cancelRedaction(note.slug) }) { Text("Keep it after all") }
        } else if (!note.redacted) {
            TextButton({ destroying = true }) { Text("Destroy") }
        }
    }

    asking?.let { ask ->
        Prompt(ask, onDone = { asking = null })
    }
    if (destroying) {
        ConfirmDestroy(note, onDismiss = { destroying = false }, onGo = {
            destroying = false
            model.redact(note.slug)
        })
    }
}

/** A question with a text answer. */
class Ask(
    val title: String,
    val hint: String,
    val initial: String,
    val then: (String) -> Unit,
)

@Composable
private fun Prompt(ask: Ask, onDone: () -> Unit) {
    var text by remember { mutableStateOf(ask.initial) }
    AlertDialog(
        onDismissRequest = onDone,
        title = { Text(ask.title) },
        text = {
            Column {
                Text(ask.hint, style = MaterialTheme.typography.labelSmall)
                TextField(text, { text = it }, Modifier.fillMaxWidth())
            }
        },
        confirmButton = { Button({ ask.then(text); onDone() }) { Text(ask.title) } },
        dismissButton = { TextButton(onDone) { Text("Cancel") } },
    )
}

/**
 * The only destructive confirmation, and it asks for the name.
 *
 * A count is dismissible in a way a typed name is not, and dismissing dialogs
 * is a reflex this one has to survive. It names what will go rather than
 * asking whether you are sure.
 */
@Composable
private fun ConfirmDestroy(note: NoteSummary, onDismiss: () -> Unit, onGo: () -> Unit) {
    var typed by remember { mutableStateOf("") }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Destroy this content?") },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                Text(note.slug, style = MaterialTheme.typography.titleSmall)
                note.text?.let { Text(it.lineSequence().first()) }
                Text(
                    "This cannot be undone once the window elapses. " +
                        "Type the note's name to confirm.",
                    style = MaterialTheme.typography.labelSmall,
                )
                TextField(typed, { typed = it }, Modifier.fillMaxWidth(), singleLine = true)
            }
        },
        confirmButton = {
            Button(onClick = onGo, enabled = typed.trim() == note.slug) { Text("Destroy") }
        },
        dismissButton = { TextButton(onDismiss) { Text("Keep it") } },
    )
}
